// SPDX-License-Identifier: AGPL-3.0-only

use std::{
    collections::{HashMap, VecDeque},
    ffi::c_void,
    mem::ManuallyDrop,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
};

use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Media::MediaFoundation::{
            CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI, IMFActivate,
            IMFAttributes, IMFByteStream, IMFMediaEventGenerator, IMFMediaSource, IMFMediaType,
            IMFSample, IMFSinkWriter, IMFSourceReader, IMFTransform, METransformHaveOutput,
            METransformNeedInput, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_E_NO_EVENTS_AVAILABLE,
            MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
            MF_EVENT_FLAG_NO_WAIT, MF_MPEG4SINK_SPSPPS_PASSTHROUGH, MF_MT_AVG_BITRATE,
            MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
            MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
            MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SINK_WRITER_DISABLE_THROTTLING,
            MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_SOURCE_READERF_ENDOFSTREAM, MF_TRANSFORM_ASYNC,
            MF_TRANSFORM_ASYNC_UNLOCK, MF_VERSION, MFCreateAttributes, MFCreateDeviceSource,
            MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL,
            MFCreateSourceReaderFromMediaSource, MFMediaType_Video, MFSTARTUP_FULL,
            MFSampleExtension_CleanPoint, MFShutdown, MFStartup, MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
            MFT_ENUM_FLAG_SYNCMFT, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_INFO,
            MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFTEnumEx,
            MFVideoFormat_H264, MFVideoFormat_I420, MFVideoFormat_IYUV, MFVideoFormat_NV12,
            MFVideoFormat_YUY2, MFVideoFormat_YV12, MFVideoInterlace_Progressive,
        },
        System::{
            Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
            Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            Variant::VARIANT,
        },
    },
    core::{Error as WindowsError, HSTRING, Interface},
};

use super::{CaptureCommand, CaptureError, CaptureStats};
use crate::encoding::{EncodingBackend, EncodingProfile};

const VIDEO_STREAM: u32 = u32::from_ne_bytes(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.to_ne_bytes());
const OUTPUT_HOLDBACK_DURATION_NS: i64 = 2_000_000_000;
const MAX_PENDING_OUTPUT_PACKETS: usize = 180;
const MAX_GOP_PACKETS: usize = 600;
const WRITER_QUEUE_CAPACITY: usize = 180;
const CLOCK_CORRELATION_TOLERANCE_NS: i64 = 10_000_000_000;

struct PlatformGuard {
    uninitialize_com: bool,
    media_foundation_started: bool,
}

impl PlatformGuard {
    fn initialize() -> Result<Self, CaptureError> {
        // SAFETY: COM is initialized and uninitialized on the current worker thread only.
        let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let uninitialize_com = if com_result.is_ok() {
            true
        } else if com_result == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(WindowsError::from_hresult(com_result).into());
        };
        // SAFETY: This call is balanced by MFShutdown in Drop.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }?;
        Ok(Self {
            uninitialize_com,
            media_foundation_started: true,
        })
    }
}

impl Drop for PlatformGuard {
    fn drop(&mut self) {
        if self.media_foundation_started {
            // SAFETY: MFStartup succeeded on this thread.
            let _ = unsafe { MFShutdown() };
        }
        if self.uninitialize_com {
            // SAFETY: CoInitializeEx succeeded on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

struct MediaSourceGuard(IMFMediaSource);

impl Drop for MediaSourceGuard {
    fn drop(&mut self) {
        // SAFETY: The source is no longer used after this point.
        let _ = unsafe { self.0.Shutdown() };
    }
}

#[derive(Clone, Copy)]
struct FormatInfo {
    width: u32,
    height: u32,
    frame_rate_numerator: u32,
    frame_rate_denominator: u32,
}

#[derive(Clone)]
struct EncodedPacket {
    bytes: Arc<[u8]>,
    source_timestamp: i64,
    monotonic_time_ns: i64,
    duration: Option<i64>,
    clean_point: bool,
}

#[derive(Clone)]
struct WriterMediaConfig {
    format: FormatInfo,
    profile: EncodingProfile,
    sequence_header: Vec<u8>,
}

struct PendingPacket {
    packet: EncodedPacket,
    from_gop_buffer: bool,
}

enum WriterMessage {
    Packet(EncodedPacket),
    Finish {
        logical_stop_monotonic_time_ns: i64,
        response: SyncSender<Result<CaptureStats, String>>,
    },
    Abort {
        response: Option<SyncSender<Result<CaptureStats, String>>>,
    },
}

struct WriterEndpoint {
    recording_id: String,
    sender: SyncSender<WriterMessage>,
    status: Receiver<String>,
    dropped_packets: Arc<AtomicU64>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WriterEndpoint {
    fn send_packet(&self, packet: EncodedPacket) -> Result<(), CaptureError> {
        if let Ok(error) = self.status.try_recv() {
            return Err(CaptureError::WriterFailed(error));
        }
        match self.sender.try_send(WriterMessage::Packet(packet)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.dropped_packets.fetch_add(1, Ordering::Relaxed);
                Err(CaptureError::WriterQueueFull(self.recording_id.clone()))
            }
            Err(TrySendError::Disconnected(_)) => Err(CaptureError::WriterFailed(
                self.status
                    .try_recv()
                    .unwrap_or_else(|_| format!("writer for {} disconnected", self.recording_id)),
            )),
        }
    }

    fn increment_dropped(&self) {
        self.dropped_packets.fetch_add(1, Ordering::Relaxed);
    }

    fn finish(mut self, logical_stop_monotonic_time_ns: i64) -> Result<CaptureStats, CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::Finish {
                logical_stop_monotonic_time_ns,
                response: response_sender,
            })
            .map_err(|_| {
                CaptureError::WriterFailed(
                    self.status.try_recv().unwrap_or_else(|_| {
                        format!("writer for {} disconnected", self.recording_id)
                    }),
                )
            })?;
        let result = response_receiver
            .recv()
            .map_err(|_| CaptureError::WriterFailed("writer finalization disconnected".into()))?
            .map_err(CaptureError::WriterFailed);
        if let Some(worker) = self.thread.take() {
            worker.join().map_err(|_| CaptureError::ThreadPanicked)?;
        }
        result
    }

    fn abort(mut self) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WriterMessage::Abort {
                response: Some(response_sender),
            })
            .map_err(|_| {
                CaptureError::WriterFailed(
                    self.status.try_recv().unwrap_or_else(|_| {
                        format!("writer for {} disconnected", self.recording_id)
                    }),
                )
            })?;
        let result = response_receiver
            .recv()
            .map_err(|_| CaptureError::WriterFailed("writer abort disconnected".into()))?
            .map(drop)
            .map_err(CaptureError::WriterFailed);
        if let Some(worker) = self.thread.take() {
            worker.join().map_err(|_| CaptureError::ThreadPanicked)?;
        }
        result
    }
}

struct ActiveMuxer {
    writer: IMFSinkWriter,
    stream: u32,
    first_timestamp: Option<i64>,
    pending_packets: VecDeque<PendingPacket>,
    stats: CaptureStats,
    dropped_packets: Arc<AtomicU64>,
    logical_start_monotonic_time_ns: i64,
    recent_written_timestamps: VecDeque<i64>,
}

struct H264Encoder {
    transform: IMFTransform,
    output_type: IMFMediaType,
    output_info: MFT_OUTPUT_STREAM_INFO,
    output_buffer_size: u32,
    event_generator: Option<IMFMediaEventGenerator>,
    input_requests: u32,
    backend: EncodingBackend,
    codec_api: Option<ICodecAPI>,
    force_keyframe_interval_hns: i64,
    next_forced_keyframe_hns: Option<i64>,
}

struct SharedEncoder {
    encoder: H264Encoder,
    format: FormatInfo,
    profile: EncodingProfile,
    gop_ring: VecDeque<EncodedPacket>,
    writers: HashMap<String, WriterEndpoint>,
    last_packet_monotonic_time_ns: Option<i64>,
}

struct PendingEnd {
    profile: EncodingProfile,
    logical_stop_monotonic_time_ns: i64,
    response: SyncSender<Result<CaptureStats, CaptureError>>,
}

struct QpcClock {
    frequency: i64,
}

impl QpcClock {
    fn initialize() -> Result<Self, CaptureError> {
        let mut frequency = 0_i64;
        // SAFETY: The output pointer is valid for the duration of the call.
        unsafe { QueryPerformanceFrequency(&raw mut frequency) }?;
        Ok(Self { frequency })
    }

    fn now_ns(&self) -> Result<i64, CaptureError> {
        let mut counter = 0_i64;
        // SAFETY: The output pointer is valid for the duration of the call.
        unsafe { QueryPerformanceCounter(&raw mut counter) }?;
        let nanoseconds = i128::from(counter) * 1_000_000_000 / i128::from(self.frequency);
        Ok(i64::try_from(nanoseconds).unwrap_or(i64::MAX))
    }
}

fn packed_pair(first: u32, second: u32) -> u64 {
    (u64::from(first) << 32) | u64::from(second)
}

fn unpacked_pair(value: u64) -> (u32, u32) {
    (
        u32::try_from(value >> 32).expect("upper half always fits in u32"),
        u32::try_from(value & u64::from(u32::MAX)).expect("lower half always fits in u32"),
    )
}

fn create_media_source(source_id: &str) -> Result<IMFMediaSource, CaptureError> {
    let mut attributes: Option<IMFAttributes> = None;
    // SAFETY: The output pointer is valid and receives an owned interface.
    unsafe { MFCreateAttributes(&raw mut attributes, 2) }?;
    let attributes = attributes.ok_or(CaptureError::NoSupportedFormat)?;
    // SAFETY: GUID pointers are static and the HSTRING remains alive for the call.
    unsafe {
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;
        attributes.SetString(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            &HSTRING::from(source_id),
        )?;
        Ok(MFCreateDeviceSource(&attributes)?)
    }
}

fn choose_input_type(reader: &IMFSourceReader) -> Result<(IMFMediaType, FormatInfo), CaptureError> {
    let supported_subtypes = [
        MFVideoFormat_NV12,
        MFVideoFormat_YUY2,
        MFVideoFormat_I420,
        MFVideoFormat_IYUV,
        MFVideoFormat_YV12,
    ];
    let mut best = None;
    for index in 0..256_u32 {
        // SAFETY: The source reader owns all returned media-type interfaces.
        let Ok(media_type) = (unsafe { reader.GetNativeMediaType(VIDEO_STREAM, index) }) else {
            break;
        };
        // SAFETY: These attributes are present on camera video media types.
        let major = unsafe { media_type.GetGUID(&MF_MT_MAJOR_TYPE) }?;
        let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }?;
        if major != MFMediaType_Video || !supported_subtypes.contains(&subtype) {
            continue;
        }
        let (width, height) = unpacked_pair(unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }?);
        let (rate_num, rate_den) =
            unpacked_pair(unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }?);
        if width == 0 || height == 0 || rate_num == 0 || rate_den == 0 {
            continue;
        }
        let pixels = u64::from(width) * u64::from(height);
        let fps_milli = u64::from(rate_num) * 1000 / u64::from(rate_den);
        if width > 1280 || height > 720 || fps_milli > 30_000 {
            continue;
        }
        let score = pixels * 100_000 + fps_milli;
        if best
            .as_ref()
            .is_none_or(|(_, _, best_score)| score > *best_score)
        {
            best = Some((
                media_type,
                FormatInfo {
                    width,
                    height,
                    frame_rate_numerator: rate_num,
                    frame_rate_denominator: rate_den,
                },
                score,
            ));
        }
    }
    best.map(|(media_type, format, _)| (media_type, format))
        .ok_or(CaptureError::NoSupportedFormat)
}

fn make_output_type(
    format: FormatInfo,
    profile: EncodingProfile,
    input_type: Option<&IMFMediaType>,
) -> Result<IMFMediaType, CaptureError> {
    // SAFETY: MFCreateMediaType returns an owned, initially empty media type.
    let output = unsafe { MFCreateMediaType() }?;
    if let Some(input_type) = input_type {
        // SAFETY: Copying the input video attributes preserves its nominal range, transfer
        // function, primaries, and YUV matrix for strict hardware MFT negotiation.
        unsafe { input_type.CopyAllItems(&output) }?;
    }
    // SAFETY: Values follow the Media Foundation H.264 output media-type requirements.
    unsafe {
        output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        output.SetUINT32(&MF_MT_AVG_BITRATE, profile.bit_rate)?;
        output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        output.SetUINT64(&MF_MT_FRAME_SIZE, packed_pair(format.width, format.height))?;
        output.SetUINT64(
            &MF_MT_FRAME_RATE,
            packed_pair(format.frame_rate_numerator, format.frame_rate_denominator),
        )?;
        output.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, packed_pair(1, 1))?;
    }
    Ok(output)
}

fn enumerate_encoder_activations(flags: MFT_ENUM_FLAG) -> Result<Vec<IMFActivate>, CaptureError> {
    let output_info = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut raw_activations: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0_u32;
    // SAFETY: Output pointers are valid. Media Foundation allocates the returned activation array.
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            None,
            Some(&raw const output_info),
            &raw mut raw_activations,
            &raw mut count,
        )?;
    }
    if raw_activations.is_null() || count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: MFTEnumEx returned count initialized Option<IMFActivate> entries.
    let entries = unsafe {
        std::slice::from_raw_parts_mut(
            raw_activations,
            usize::try_from(count).expect("u32 count fits in usize"),
        )
    };
    let activations = entries.iter_mut().filter_map(Option::take).collect();
    // SAFETY: This frees only the outer array; each interface was moved into activations.
    unsafe { CoTaskMemFree(Some(raw_activations.cast::<c_void>())) };
    Ok(activations)
}

impl H264Encoder {
    fn create(
        input_type: &IMFMediaType,
        format: FormatInfo,
        profile: EncodingProfile,
    ) -> Result<Self, CaptureError> {
        let mut candidates = Vec::new();
        if profile.prefer_hardware {
            candidates.push((
                MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0),
                EncodingBackend::Hardware,
            ));
        }
        candidates.push((
            MFT_ENUM_FLAG(MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0),
            EncodingBackend::Software,
        ));

        let output_type = make_output_type(format, profile, Some(input_type))?;
        let mut last_error = None;
        for (flags, backend) in candidates {
            for activation in enumerate_encoder_activations(flags)? {
                // SAFETY: The activation was returned for the H.264 encoder category.
                let transform = match unsafe { activation.ActivateObject::<IMFTransform>() } {
                    Ok(transform) => transform,
                    Err(error) => {
                        last_error = Some(error);
                        continue;
                    }
                };
                match Self::configure(
                    transform,
                    input_type,
                    &output_type,
                    format,
                    profile,
                    backend,
                ) {
                    Ok(encoder) => return Ok(encoder),
                    Err(CaptureError::Windows(error)) => last_error = Some(error),
                    Err(error) => return Err(error),
                }
            }
        }
        last_error.map_or(Err(CaptureError::NoEncoder), |error| Err(error.into()))
    }

    fn configure(
        transform: IMFTransform,
        input_type: &IMFMediaType,
        output_type: &IMFMediaType,
        format: FormatInfo,
        profile: EncodingProfile,
        backend: EncodingBackend,
    ) -> Result<Self, CaptureError> {
        // SAFETY: Transform attributes and media types are configured before streaming starts.
        let asynchronous = unsafe { transform.GetAttributes() }
            .ok()
            .and_then(|attributes| unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.ok())
            .is_some_and(|value| value != 0);
        if asynchronous {
            let attributes = unsafe { transform.GetAttributes() }?;
            unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }?;
        }
        unsafe {
            transform.SetOutputType(0, output_type, 0)?;
            transform.SetInputType(0, input_type, 0)?;
        }

        let codec_api = transform.cast::<ICodecAPI>().ok();
        if let Some(codec_api) = &codec_api {
            let fps = u64::from(format.frame_rate_numerator)
                / u64::from(format.frame_rate_denominator.max(1));
            let gop_frames = u32::try_from(
                ((fps * u64::from(profile.gop_duration_ms)) / 1_000).clamp(1, u64::from(u32::MAX)),
            )
            .expect("the GOP frame count was clamped to u32");
            let value = VARIANT::from(gop_frames);
            // SAFETY: The variant contains a VT_UI4 value accepted by the GOP-size codec property.
            let _ = unsafe { codec_api.SetValue(&CODECAPI_AVEncMPVGOPSize, &raw const value) };
        }

        // SAFETY: Media types are set and these notifications start the MFT streaming lifecycle.
        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }
        let output_info = unsafe { transform.GetOutputStreamInfo(0) }?;
        let raw_frame_bytes = u64::from(format.width) * u64::from(format.height) * 2;
        let output_buffer_size = output_info
            .cbSize
            .max(u32::try_from(raw_frame_bytes.min(u64::from(u32::MAX))).unwrap_or(u32::MAX));
        let event_generator = if asynchronous {
            Some(transform.cast::<IMFMediaEventGenerator>()?)
        } else {
            None
        };
        Ok(Self {
            transform,
            output_type: output_type.clone(),
            output_info,
            output_buffer_size,
            event_generator,
            input_requests: 0,
            backend,
            codec_api,
            force_keyframe_interval_hns: i64::from(profile.gop_duration_ms) * 10_000,
            next_forced_keyframe_hns: None,
        })
    }

    fn process_sample(&mut self, sample: &IMFSample) -> Result<Vec<IMFSample>, CaptureError> {
        self.maybe_force_keyframe(sample);
        if self.event_generator.is_some() {
            let mut output = self.poll_async_events()?;
            if self.input_requests > 0 {
                // SAFETY: A NeedInput event authorizes exactly one ProcessInput call.
                unsafe { self.transform.ProcessInput(0, sample, 0) }?;
                self.input_requests -= 1;
            }
            output.extend(self.poll_async_events()?);
            return Ok(output);
        }

        // SAFETY: The synchronous MFT accepts one sample at a time. If it is full, drain first.
        if let Err(error) = unsafe { self.transform.ProcessInput(0, sample, 0) } {
            if error.code() != MF_E_NOTACCEPTING {
                return Err(error.into());
            }
            let mut output = self.drain_sync_output()?;
            unsafe { self.transform.ProcessInput(0, sample, 0) }?;
            output.extend(self.drain_sync_output()?);
            return Ok(output);
        }
        self.drain_sync_output()
    }

    fn maybe_force_keyframe(&mut self, sample: &IMFSample) {
        let Ok(timestamp) = (unsafe { sample.GetSampleTime() }) else {
            return;
        };
        let next = self
            .next_forced_keyframe_hns
            .unwrap_or_else(|| timestamp.saturating_add(self.force_keyframe_interval_hns));
        if timestamp < next {
            self.next_forced_keyframe_hns = Some(next);
            return;
        }
        self.force_next_keyframe();
        self.next_forced_keyframe_hns =
            Some(timestamp.saturating_add(self.force_keyframe_interval_hns));
    }

    fn force_next_keyframe(&self) {
        let Some(codec_api) = &self.codec_api else {
            return;
        };
        let value = VARIANT::from(true);
        // SAFETY: The variant contains VT_BOOL, the type required by ForceKeyFrame.
        let _ = unsafe { codec_api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &raw const value) };
    }

    fn writer_media_config(
        &self,
        format: FormatInfo,
        profile: EncodingProfile,
    ) -> WriterMediaConfig {
        let sequence_header = (|| {
            let size = unsafe { self.output_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }.ok()?;
            let mut bytes = vec![0_u8; usize::try_from(size).ok()?];
            unsafe {
                self.output_type
                    .GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut bytes, None)
            }
            .ok()?;
            Some(bytes)
        })()
        .unwrap_or_default();
        WriterMediaConfig {
            format,
            profile,
            sequence_header,
        }
    }

    fn poll_async_events(&mut self) -> Result<Vec<IMFSample>, CaptureError> {
        let mut output = Vec::new();
        let generator = self
            .event_generator
            .as_ref()
            .expect("async polling requires an event generator")
            .clone();
        loop {
            // SAFETY: NO_WAIT makes this polling operation non-blocking.
            let event = match unsafe { generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                Err(error) => return Err(error.into()),
            };
            // SAFETY: GetType only reads the event type.
            match unsafe { event.GetType() }? {
                value if value == METransformNeedInput.0 as u32 => {
                    self.input_requests = self.input_requests.saturating_add(1);
                }
                value if value == METransformHaveOutput.0 as u32 => {
                    if let Some(sample) = self.take_output_sample()? {
                        output.push(sample);
                    }
                }
                _ => {}
            }
        }
        Ok(output)
    }

    fn drain_sync_output(&mut self) -> Result<Vec<IMFSample>, CaptureError> {
        let mut output = Vec::new();
        while let Some(sample) = self.take_output_sample()? {
            output.push(sample);
        }
        Ok(output)
    }

    fn take_output_sample(&mut self) -> Result<Option<IMFSample>, CaptureError> {
        let transform_provides_sample =
            self.output_info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        let supplied_sample = if transform_provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample() }?;
            let buffer = unsafe { MFCreateMemoryBuffer(self.output_buffer_size.max(1)) }?;
            unsafe { sample.AddBuffer(&buffer) }?;
            Some(sample)
        };
        let mut output_buffer = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(supplied_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0_u32;
        // SAFETY: The output array and status pointer remain valid for the call.
        let result = unsafe {
            self.transform.ProcessOutput(
                0,
                std::slice::from_mut(&mut output_buffer),
                &raw mut status,
            )
        };
        // SAFETY: These fields are ManuallyDrop solely to match the COM ABI; ownership is recovered
        // exactly once after ProcessOutput returns.
        let sample = unsafe { ManuallyDrop::take(&mut output_buffer.pSample) };
        let events = unsafe { ManuallyDrop::take(&mut output_buffer.pEvents) };
        drop(events);
        match result {
            Ok(()) => Ok(sample),
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                // SAFETY: A stream-change result requires selecting one of the newly advertised
                // output types before processing can continue.
                let output_type = unsafe { self.transform.GetOutputAvailableType(0, 0) }?;
                unsafe { self.transform.SetOutputType(0, &output_type, 0) }?;
                self.output_type = output_type;
                self.output_info = unsafe { self.transform.GetOutputStreamInfo(0) }?;
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for H264Encoder {
    fn drop(&mut self) {
        // SAFETY: These messages terminate the MFT lifecycle; failures during Drop are non-fatal.
        let _ = unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
        };
        // There is no consumer left during Drop, so discard queued output instead of issuing a
        // drain command whose contract requires processing every remaining output sample.
        let _ = unsafe { self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) };
        let _ = unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0)
        };
    }
}

fn copy_encoded_packet(
    sample: &IMFSample,
    source_anchor: i64,
    monotonic_anchor: i64,
) -> Result<EncodedPacket, CaptureError> {
    let source_timestamp = unsafe { sample.GetSampleTime() }?;
    let buffer = unsafe { sample.ConvertToContiguousBuffer() }?;
    let mut data = std::ptr::null_mut();
    let mut current_length = 0_u32;
    unsafe { buffer.Lock(&raw mut data, None, Some(&raw mut current_length)) }?;
    let bytes: Arc<[u8]> = if current_length == 0 {
        Arc::from([])
    } else {
        // SAFETY: Lock returned current_length readable bytes.
        unsafe {
            std::slice::from_raw_parts(
                data.cast_const(),
                usize::try_from(current_length).expect("u32 length fits in usize"),
            )
        }
        .into()
    };
    unsafe { buffer.Unlock() }?;
    let source_delta_ns = source_timestamp
        .saturating_sub(source_anchor)
        .saturating_mul(100);
    Ok(EncodedPacket {
        bytes,
        source_timestamp,
        monotonic_time_ns: monotonic_anchor.saturating_add(source_delta_ns),
        duration: unsafe { sample.GetSampleDuration() }.ok(),
        clean_point: unsafe { sample.GetUINT32(&MFSampleExtension_CleanPoint) }
            .is_ok_and(|value| value != 0),
    })
}

fn create_muxer(
    output_path: &Path,
    encoded_type: &IMFMediaType,
    backend: EncodingBackend,
    dropped_packets: Arc<AtomicU64>,
    logical_start_monotonic_time_ns: i64,
) -> Result<ActiveMuxer, CaptureError> {
    let mut attributes = None;
    unsafe { MFCreateAttributes(&raw mut attributes, 3) }?;
    let attributes = attributes.ok_or(CaptureError::NoEncoder)?;
    unsafe {
        attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1)?;
        attributes.SetUINT32(&MF_MPEG4SINK_SPSPPS_PASSTHROUGH, 1)?;
        attributes.SetUINT32(
            &MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
            u32::from(backend == EncodingBackend::Hardware),
        )?;
    }
    let output_url = HSTRING::from(output_path.to_string_lossy().as_ref());
    let writer = unsafe {
        MFCreateSinkWriterFromURL(&output_url, None::<&IMFByteStream>, Some(&attributes))
    }?;
    let stream = unsafe { writer.AddStream(encoded_type) }?;
    unsafe {
        writer.SetInputMediaType(stream, encoded_type, None::<&IMFAttributes>)?;
        writer.BeginWriting()?;
    }
    Ok(ActiveMuxer {
        writer,
        stream,
        first_timestamp: None,
        pending_packets: VecDeque::new(),
        stats: CaptureStats {
            encoding_backend: backend,
            ..CaptureStats::default()
        },
        dropped_packets,
        logical_start_monotonic_time_ns,
        recent_written_timestamps: VecDeque::new(),
    })
}

fn write_packet(active: &mut ActiveMuxer, pending: PendingPacket) -> Result<(), CaptureError> {
    let packet = pending.packet;
    let length = u32::try_from(packet.bytes.len()).map_err(|_| CaptureError::FrameTooLarge)?;
    let sample = unsafe { MFCreateSample() }?;
    let buffer = unsafe { MFCreateMemoryBuffer(length) }?;
    let mut destination = std::ptr::null_mut();
    unsafe { buffer.Lock(&raw mut destination, None, None) }?;
    if !packet.bytes.is_empty() {
        // SAFETY: The destination was allocated with packet.bytes.len() bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(packet.bytes.as_ptr(), destination, packet.bytes.len());
        }
    }
    unsafe {
        buffer.Unlock()?;
        buffer.SetCurrentLength(length)?;
        sample.AddBuffer(&buffer)?;
        let origin = *active
            .first_timestamp
            .get_or_insert(packet.source_timestamp);
        sample.SetSampleTime(packet.source_timestamp.saturating_sub(origin))?;
        if let Some(duration) = packet.duration {
            sample.SetSampleDuration(duration)?;
        }
        if packet.clean_point {
            sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
        }
        active.writer.WriteSample(active.stream, &sample)?;
    }
    active
        .stats
        .first_frame_monotonic_time_ns
        .get_or_insert(packet.monotonic_time_ns);
    active.stats.last_frame_monotonic_time_ns = Some(packet.monotonic_time_ns);
    if packet.monotonic_time_ns >= active.logical_start_monotonic_time_ns {
        active
            .stats
            .drift_start_frame_monotonic_time_ns
            .get_or_insert(packet.monotonic_time_ns);
    }
    active
        .recent_written_timestamps
        .push_back(packet.monotonic_time_ns);
    while active.recent_written_timestamps.len() > MAX_GOP_PACKETS {
        active.recent_written_timestamps.pop_front();
    }
    active.stats.frame_count += 1;
    if pending.from_gop_buffer {
        active.stats.buffered_frame_count += 1;
    }
    Ok(())
}

fn flush_aged_packets(active: &mut ActiveMuxer) -> Result<(), CaptureError> {
    loop {
        let should_flush = active.pending_packets.len() > MAX_PENDING_OUTPUT_PACKETS
            || active.pending_packets.front().is_some_and(|oldest| {
                active.pending_packets.back().is_some_and(|newest| {
                    newest
                        .packet
                        .monotonic_time_ns
                        .saturating_sub(oldest.packet.monotonic_time_ns)
                        > OUTPUT_HOLDBACK_DURATION_NS
                })
            });
        if !should_flush {
            return Ok(());
        }
        let packet = active
            .pending_packets
            .pop_front()
            .expect("a flushable queue is non-empty");
        write_packet(active, packet)?;
    }
}

fn finalize_muxer(
    mut active: ActiveMuxer,
    logical_stop_monotonic_time_ns: Option<i64>,
) -> Result<CaptureStats, CaptureError> {
    if let Some(logical_stop) = logical_stop_monotonic_time_ns {
        active.stats.stop_holdback_exceeded = active
            .stats
            .last_frame_monotonic_time_ns
            .is_some_and(|last_written| last_written > logical_stop);
        while active
            .pending_packets
            .front()
            .is_some_and(|pending| pending.packet.monotonic_time_ns <= logical_stop)
        {
            let packet = active
                .pending_packets
                .pop_front()
                .expect("the pending queue was checked as non-empty");
            write_packet(&mut active, packet)?;
        }
        active.stats.discarded_tail_frame_count = active.pending_packets.len() as u64;
        active.pending_packets.clear();
        active.stats.drift_end_frame_monotonic_time_ns = active
            .recent_written_timestamps
            .iter()
            .rev()
            .find(|timestamp| **timestamp <= logical_stop)
            .copied();
    } else {
        while let Some(packet) = active.pending_packets.pop_front() {
            write_packet(&mut active, packet)?;
        }
    }
    active.stats.dropped_frame_count = active.dropped_packets.load(Ordering::Relaxed);
    // SAFETY: BeginWriting succeeded and no samples are written after Finalize.
    unsafe { active.writer.Finalize() }?;
    Ok(active.stats)
}

struct WriterThreadArgs {
    output_path: std::path::PathBuf,
    media: WriterMediaConfig,
    backend: EncodingBackend,
    initial_gop: Vec<EncodedPacket>,
    messages: Receiver<WriterMessage>,
    ready: SyncSender<Result<(), String>>,
    status: SyncSender<String>,
    dropped_packets: Arc<AtomicU64>,
    logical_start_monotonic_time_ns: i64,
}

#[allow(clippy::needless_pass_by_value)]
fn writer_thread(args: WriterThreadArgs) {
    let initialized = (|| {
        let platform = PlatformGuard::initialize()?;
        let encoded_type = make_output_type(args.media.format, args.media.profile, None)?;
        if !args.media.sequence_header.is_empty() {
            unsafe {
                encoded_type.SetBlob(
                    &MF_MT_MPEG_SEQUENCE_HEADER,
                    args.media.sequence_header.as_slice(),
                )
            }?;
        }
        let mut muxer = create_muxer(
            &args.output_path,
            &encoded_type,
            args.backend,
            Arc::clone(&args.dropped_packets),
            args.logical_start_monotonic_time_ns,
        )?;
        muxer
            .pending_packets
            .extend(
                args.initial_gop
                    .iter()
                    .cloned()
                    .map(|packet| PendingPacket {
                        packet,
                        from_gop_buffer: true,
                    }),
            );
        Ok::<_, CaptureError>((platform, muxer))
    })();
    let (platform, mut muxer) = match initialized {
        Ok(state) => {
            let _ = args.ready.send(Ok(()));
            state
        }
        Err(error) => {
            let message = error.to_string();
            let _ = args.ready.send(Err(message.clone()));
            let _ = args.status.send(message);
            return;
        }
    };

    loop {
        match args.messages.recv() {
            Ok(WriterMessage::Packet(packet)) => {
                muxer.pending_packets.push_back(PendingPacket {
                    packet,
                    from_gop_buffer: false,
                });
                if let Err(error) = flush_aged_packets(&mut muxer) {
                    let _ = args.status.send(error.to_string());
                    break;
                }
            }
            Ok(WriterMessage::Finish {
                logical_stop_monotonic_time_ns,
                response,
            }) => {
                let result = finalize_muxer(muxer, Some(logical_stop_monotonic_time_ns))
                    .map_err(|error| error.to_string());
                let _ = response.send(result);
                drop(platform);
                return;
            }
            Ok(WriterMessage::Abort { response }) => {
                let result = finalize_muxer(muxer, None).map_err(|error| error.to_string());
                if let Some(response) = response {
                    let _ = response.send(result);
                }
                drop(platform);
                return;
            }
            Err(_) => {
                let _ = finalize_muxer(muxer, None);
                drop(platform);
                return;
            }
        }
    }
}

fn spawn_writer(
    recording_id: String,
    output_path: &Path,
    media: WriterMediaConfig,
    backend: EncodingBackend,
    initial_gop: Vec<EncodedPacket>,
    logical_start_monotonic_time_ns: i64,
) -> Result<WriterEndpoint, CaptureError> {
    let (sender, receiver) = mpsc::sync_channel(WRITER_QUEUE_CAPACITY);
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let (status_sender, status_receiver) = mpsc::sync_channel(1);
    let dropped_packets = Arc::new(AtomicU64::new(0));
    let worker_drops = Arc::clone(&dropped_packets);
    let worker_path = output_path.to_owned();
    let thread = thread::Builder::new()
        .name(format!(
            "DAWVR Muxer {}",
            &recording_id[..recording_id.len().min(8)]
        ))
        .spawn(move || {
            writer_thread(WriterThreadArgs {
                output_path: worker_path,
                media,
                backend,
                initial_gop,
                messages: receiver,
                ready: ready_sender,
                status: status_sender,
                dropped_packets: worker_drops,
                logical_start_monotonic_time_ns,
            });
        })
        .map_err(|_| CaptureError::ThreadPanicked)?;
    ready_receiver
        .recv()
        .map_err(|_| CaptureError::WriterFailed("writer initialization disconnected".into()))?
        .map_err(CaptureError::WriterFailed)?;
    Ok(WriterEndpoint {
        recording_id,
        sender,
        status: status_receiver,
        dropped_packets,
        thread: Some(thread),
    })
}

impl SharedEncoder {
    fn create(
        input_type: &IMFMediaType,
        format: FormatInfo,
        profile: EncodingProfile,
    ) -> Result<Self, CaptureError> {
        Ok(Self {
            encoder: H264Encoder::create(input_type, format, profile)?,
            format,
            profile,
            gop_ring: VecDeque::new(),
            writers: HashMap::new(),
            last_packet_monotonic_time_ns: None,
        })
    }

    fn begin(
        &mut self,
        recording_id: String,
        output_path: &Path,
        logical_start_monotonic_time_ns: i64,
    ) -> Result<(), CaptureError> {
        if self.writers.contains_key(&recording_id) {
            return Err(CaptureError::AlreadyRecording);
        }
        let clock_correlated = self.gop_ring.back().is_some_and(|packet| {
            packet
                .monotonic_time_ns
                .abs_diff(logical_start_monotonic_time_ns)
                <= CLOCK_CORRELATION_TOLERANCE_NS as u64
        });
        let gop_is_recent = self.gop_ring.front().is_some_and(|packet| {
            logical_start_monotonic_time_ns.saturating_sub(packet.monotonic_time_ns)
                <= i64::from(self.profile.gop_duration_ms)
                    .saturating_mul(1_000_000)
                    .saturating_add(250_000_000)
        });
        let initial_gop = if clock_correlated && gop_is_recent {
            self.gop_ring.iter().cloned().collect()
        } else {
            self.encoder.force_next_keyframe();
            Vec::new()
        };
        let writer = spawn_writer(
            recording_id.clone(),
            output_path,
            self.encoder.writer_media_config(self.format, self.profile),
            self.encoder.backend,
            initial_gop,
            logical_start_monotonic_time_ns,
        )?;
        self.writers.insert(recording_id, writer);
        Ok(())
    }

    fn process(
        &mut self,
        sample: &IMFSample,
        source_anchor: i64,
        monotonic_anchor: i64,
    ) -> Result<Vec<(String, CaptureError)>, CaptureError> {
        let samples = self.encoder.process_sample(sample)?;
        let mut failed = Vec::new();
        for sample in samples {
            let packet = copy_encoded_packet(&sample, source_anchor, monotonic_anchor)?;
            self.last_packet_monotonic_time_ns = Some(packet.monotonic_time_ns);
            if packet.clean_point {
                self.gop_ring.clear();
            }
            self.gop_ring.push_back(packet.clone());
            while self.gop_ring.len() > MAX_GOP_PACKETS {
                self.gop_ring.pop_front();
            }
            let recording_ids: Vec<_> = self.writers.keys().cloned().collect();
            for recording_id in recording_ids {
                let result = self
                    .writers
                    .get(&recording_id)
                    .expect("recording ID came from the writer map")
                    .send_packet(packet.clone());
                if let Err(error) = result {
                    if let Some(writer) = self.writers.remove(&recording_id) {
                        thread::spawn(move || {
                            let _ = writer.abort();
                        });
                    }
                    failed.push((recording_id, error));
                }
            }
        }
        Ok(failed)
    }

    fn increment_dropped(&self) {
        for writer in self.writers.values() {
            writer.increment_dropped();
        }
    }
}

fn finalize_endpoint_async(
    writer: WriterEndpoint,
    logical_stop_monotonic_time_ns: i64,
    response: SyncSender<Result<CaptureStats, CaptureError>>,
) {
    thread::spawn(move || {
        let _ = response.send(writer.finish(logical_stop_monotonic_time_ns));
    });
}

fn abort_endpoint_async(
    writer: Option<WriterEndpoint>,
    response: SyncSender<Result<(), CaptureError>>,
) {
    thread::spawn(move || {
        let result = writer.map_or(Ok(()), WriterEndpoint::abort);
        let _ = response.send(result);
    });
}

pub fn run(
    source_id: &str,
    commands: &Receiver<CaptureCommand>,
    ready: &SyncSender<Result<(), CaptureError>>,
) -> Result<(), CaptureError> {
    let initialized = (|| {
        let platform = PlatformGuard::initialize()?;
        let source = MediaSourceGuard(create_media_source(source_id)?);
        let reader =
            unsafe { MFCreateSourceReaderFromMediaSource(&source.0, None::<&IMFAttributes>) }?;
        let (input_type, format) = choose_input_type(&reader)?;
        unsafe { reader.SetCurrentMediaType(VIDEO_STREAM, None, &input_type) }?;
        let clock = QpcClock::initialize()?;
        Ok((platform, source, reader, input_type, format, clock))
    })();

    let (platform, source, reader, input_type, format, clock) = match initialized {
        Ok(state) => {
            tracing::info!(source_id, "capture pipeline prepared");
            let _ = ready.send(Ok(()));
            state
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return Ok(());
        }
    };

    let mut encoders: HashMap<EncodingProfile, SharedEncoder> = HashMap::new();
    let mut recording_profiles: HashMap<String, EncodingProfile> = HashMap::new();
    let mut failed_writers: HashMap<String, String> = HashMap::new();
    let mut pending_ends: HashMap<String, PendingEnd> = HashMap::new();
    let mut clock_anchor: Option<(i64, i64)> = None;

    'capture: loop {
        loop {
            match commands.try_recv() {
                Ok(CaptureCommand::PrepareProfile { profile, response }) => {
                    let result = match encoders.entry(profile) {
                        std::collections::hash_map::Entry::Occupied(_) => Ok(()),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            SharedEncoder::create(&input_type, format, profile).map(|encoder| {
                                tracing::info!(
                                    source_id,
                                    profile = %profile.stable_id(),
                                    backend = encoder.encoder.backend.as_str(),
                                    "shared encoder prepared"
                                );
                                entry.insert(encoder);
                            })
                        }
                    };
                    let _ = response.send(result);
                }
                Ok(CaptureCommand::ReleaseProfile { profile, response }) => {
                    let in_use = recording_profiles.values().any(|active| *active == profile);
                    let result = if in_use {
                        Err(CaptureError::AlreadyRecording)
                    } else {
                        encoders.remove(&profile);
                        Ok(())
                    };
                    let _ = response.send(result);
                }
                Ok(CaptureCommand::Begin {
                    recording_id,
                    output_path,
                    logical_start_monotonic_time_ns,
                    profile,
                    response,
                }) => {
                    failed_writers.remove(&recording_id);
                    let result = match recording_profiles.entry(recording_id.clone()) {
                        std::collections::hash_map::Entry::Occupied(_) => {
                            Err(CaptureError::AlreadyRecording)
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            if let Some(encoder) = encoders.get_mut(&profile) {
                                encoder
                                    .begin(
                                        recording_id,
                                        &output_path,
                                        logical_start_monotonic_time_ns,
                                    )
                                    .map(|()| {
                                        entry.insert(profile);
                                    })
                            } else {
                                Err(CaptureError::ProfileNotPrepared)
                            }
                        }
                    };
                    let _ = response.send(result);
                }
                Ok(CaptureCommand::End {
                    recording_id,
                    logical_stop_monotonic_time_ns,
                    response,
                }) => {
                    if pending_ends.contains_key(&recording_id) {
                        let _ = response.send(Err(CaptureError::AlreadyRecording));
                        continue;
                    }
                    if let Some(error) = failed_writers.remove(&recording_id) {
                        let _ = response.send(Err(CaptureError::WriterFailed(error)));
                    } else if let Some(profile) = recording_profiles.get(&recording_id).copied() {
                        let encoded_through_stop = encoders
                            .get(&profile)
                            .and_then(|encoder| encoder.last_packet_monotonic_time_ns)
                            .is_some_and(|last| last >= logical_stop_monotonic_time_ns);
                        if encoded_through_stop {
                            recording_profiles.remove(&recording_id);
                            let writer = encoders
                                .get_mut(&profile)
                                .and_then(|encoder| encoder.writers.remove(&recording_id));
                            if let Some(writer) = writer {
                                finalize_endpoint_async(
                                    writer,
                                    logical_stop_monotonic_time_ns,
                                    response,
                                );
                            } else {
                                let _ = response.send(Err(CaptureError::NotRecording));
                            }
                        } else {
                            pending_ends.insert(
                                recording_id,
                                PendingEnd {
                                    profile,
                                    logical_stop_monotonic_time_ns,
                                    response,
                                },
                            );
                        }
                    } else {
                        let _ = response.send(Err(CaptureError::NotRecording));
                    }
                }
                Ok(CaptureCommand::Abort {
                    recording_id,
                    response,
                }) => {
                    if let Some(pending) = pending_ends.remove(&recording_id) {
                        let _ = pending.response.send(Err(CaptureError::WriterFailed(
                            "recording was aborted while finalization was pending".into(),
                        )));
                    }
                    let writer = recording_profiles
                        .remove(&recording_id)
                        .and_then(|profile| encoders.get_mut(&profile))
                        .and_then(|encoder| encoder.writers.remove(&recording_id));
                    failed_writers.remove(&recording_id);
                    abort_endpoint_async(writer, response);
                }
                Ok(CaptureCommand::Shutdown) | Err(TryRecvError::Disconnected) => {
                    for (_, pending) in pending_ends.drain() {
                        let _ = pending.response.send(Err(CaptureError::WriterFailed(
                            "capture shut down while finalization was pending".into(),
                        )));
                    }
                    for encoder in encoders.values_mut() {
                        for (_, writer) in encoder.writers.drain() {
                            let _ = writer.abort();
                        }
                    }
                    break 'capture;
                }
                Err(TryRecvError::Empty) => break,
            }
        }

        let mut flags = 0_u32;
        let mut timestamp = 0_i64;
        let mut sample = None;
        // SAFETY: All optional output pointers are valid for this synchronous call.
        unsafe {
            reader.ReadSample(
                VIDEO_STREAM,
                0,
                None,
                Some(&raw mut flags),
                Some(&raw mut timestamp),
                Some(&raw mut sample),
            )?;
        }
        let arrival_monotonic_time_ns = clock.now_ns()?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            break;
        }
        let Some(sample) = sample else {
            for encoder in encoders.values() {
                encoder.increment_dropped();
            }
            continue;
        };
        let (source_anchor, monotonic_anchor) =
            *clock_anchor.get_or_insert((timestamp, arrival_monotonic_time_ns));

        let profiles: Vec<_> = encoders.keys().copied().collect();
        for profile in profiles {
            let result = encoders
                .get_mut(&profile)
                .expect("profile came from the encoder map")
                .process(&sample, source_anchor, monotonic_anchor);
            match result {
                Ok(failures) => {
                    for (recording_id, error) in failures {
                        recording_profiles.remove(&recording_id);
                        tracing::warn!(%recording_id, %error, "isolated muxer failure");
                        if let Some(pending) = pending_ends.remove(&recording_id) {
                            let _ = pending
                                .response
                                .send(Err(CaptureError::WriterFailed(error.to_string())));
                        } else {
                            failed_writers.insert(recording_id, error.to_string());
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(
                        source_id,
                        profile = %profile.stable_id(),
                        %error,
                        "shared encoder failed"
                    );
                    if let Some(mut encoder) = encoders.remove(&profile) {
                        for (recording_id, writer) in encoder.writers.drain() {
                            recording_profiles.remove(&recording_id);
                            if let Some(pending) = pending_ends.remove(&recording_id) {
                                let _ = pending
                                    .response
                                    .send(Err(CaptureError::WriterFailed(error.to_string())));
                            } else {
                                failed_writers.insert(recording_id, error.to_string());
                            }
                            thread::spawn(move || {
                                let _ = writer.abort();
                            });
                        }
                    }
                }
            }
        }

        let ready_recordings: Vec<_> = pending_ends
            .iter()
            .filter(|(_, pending)| {
                encoders
                    .get(&pending.profile)
                    .and_then(|encoder| encoder.last_packet_monotonic_time_ns)
                    .is_some_and(|last| last >= pending.logical_stop_monotonic_time_ns)
            })
            .map(|(recording_id, _)| recording_id.clone())
            .collect();
        for recording_id in ready_recordings {
            let pending = pending_ends
                .remove(&recording_id)
                .expect("recording ID came from the pending-end map");
            recording_profiles.remove(&recording_id);
            let writer = encoders
                .get_mut(&pending.profile)
                .and_then(|encoder| encoder.writers.remove(&recording_id));
            if let Some(writer) = writer {
                finalize_endpoint_async(
                    writer,
                    pending.logical_stop_monotonic_time_ns,
                    pending.response,
                );
            } else {
                let _ = pending.response.send(Err(CaptureError::NotRecording));
            }
        }
    }

    // MFTs and their COM interfaces must be released while Media Foundation is still active.
    // `platform` was initialized before `encoders`, so relying on explicit platform shutdown
    // without first clearing the map leaves encoder destructors calling into a shut-down MF.
    drop(encoders);
    drop(reader);
    drop(source);
    drop(platform);
    tracing::info!(source_id, "prepared capture stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gop_buffer_restarts_at_clean_point() {
        let packet = |time, clean_point| EncodedPacket {
            bytes: Arc::from([1_u8, 2, 3]),
            source_timestamp: time / 100,
            monotonic_time_ns: time,
            duration: Some(333_333),
            clean_point,
        };
        let mut ring = VecDeque::new();
        for value in [packet(0, true), packet(1, false), packet(2, true)] {
            if value.clean_point {
                ring.clear();
            }
            ring.push_back(value);
        }
        assert_eq!(ring.len(), 1);
        assert_eq!(ring.front().unwrap().monotonic_time_ns, 2);
    }

    #[test]
    fn writer_queue_is_bounded() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let packet = EncodedPacket {
            bytes: Arc::from([1_u8]),
            source_timestamp: 0,
            monotonic_time_ns: 0,
            duration: None,
            clean_point: true,
        };
        sender
            .try_send(WriterMessage::Packet(packet.clone()))
            .unwrap();
        assert!(matches!(
            sender.try_send(WriterMessage::Packet(packet)),
            Err(TrySendError::Full(_))
        ));
    }
}
