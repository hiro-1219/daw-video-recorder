// SPDX-License-Identifier: AGPL-3.0-only

use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureStats {
    pub frame_count: u64,
    pub dropped_frame_count: u64,
    pub buffered_frame_count: u64,
    pub first_frame_monotonic_time_ns: Option<i64>,
    pub last_frame_monotonic_time_ns: Option<i64>,
    pub discarded_tail_frame_count: u64,
    pub stop_holdback_exceeded: bool,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[cfg(windows)]
    #[error("Media Foundation capture failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("camera has no supported uncompressed video format")]
    NoSupportedFormat,
    #[error("capture initialization timed out")]
    InitializationTimeout,
    #[error("capture thread terminated before initialization completed")]
    InitializationDisconnected,
    #[error("capture thread panicked")]
    ThreadPanicked,
    #[error("capture command timed out while {0}")]
    CommandTimeout(&'static str),
    #[error("capture thread disconnected while {0}")]
    CommandDisconnected(&'static str),
    #[error("recording has already started on this prepared capture")]
    AlreadyRecording,
    #[error("recording has not started on this prepared capture")]
    NotRecording,
    #[error("captured frame is too large to store in a Media Foundation buffer")]
    FrameTooLarge,
    #[cfg(not(windows))]
    #[error("camera capture is currently supported on Windows only")]
    UnsupportedPlatform,
}

enum CaptureCommand {
    Begin {
        recording_id: String,
        output_path: PathBuf,
        logical_start_monotonic_time_ns: i64,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    End {
        recording_id: String,
        logical_stop_monotonic_time_ns: i64,
        response: mpsc::SyncSender<Result<CaptureStats, CaptureError>>,
    },
    Abort {
        recording_id: String,
        response: mpsc::SyncSender<Result<(), CaptureError>>,
    },
    Shutdown,
}

#[derive(Debug)]
pub struct CaptureHandle {
    commands: mpsc::Sender<CaptureCommand>,
    thread: Option<thread::JoinHandle<Result<(), CaptureError>>>,
}

impl CaptureHandle {
    pub fn prepare(source_id: String) -> Result<Self, CaptureError> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let capture_thread = thread::Builder::new()
            .name("DAW Video Recorder Capture".into())
            .spawn(move || platform::run(&source_id, &command_receiver, &ready_sender))
            .map_err(|_| CaptureError::ThreadPanicked)?;

        match ready_receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                commands: command_sender,
                thread: Some(capture_thread),
            }),
            Ok(Err(error)) => {
                let _ = capture_thread.join();
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = command_sender.send(CaptureCommand::Shutdown);
                let _ = capture_thread.join();
                Err(CaptureError::InitializationTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = capture_thread.join();
                Err(CaptureError::InitializationDisconnected)
            }
        }
    }

    pub fn begin_recording(
        &self,
        recording_id: String,
        output_path: PathBuf,
        logical_start_monotonic_time_ns: i64,
    ) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::Begin {
                recording_id,
                output_path,
                logical_start_monotonic_time_ns,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("starting recording"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("starting recording")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("starting recording")
                }
            })?
    }

    pub fn end_recording(
        &self,
        recording_id: String,
        logical_stop_monotonic_time_ns: i64,
    ) -> Result<CaptureStats, CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::End {
                recording_id,
                logical_stop_monotonic_time_ns,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("stopping recording"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("stopping recording")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("stopping recording")
                }
            })?
    }

    pub fn abort_recording(&self, recording_id: String) -> Result<(), CaptureError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.commands
            .send(CaptureCommand::Abort {
                recording_id,
                response: response_sender,
            })
            .map_err(|_| CaptureError::CommandDisconnected("aborting recording"))?;
        response_receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    CaptureError::CommandTimeout("aborting recording")
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    CaptureError::CommandDisconnected("aborting recording")
                }
            })?
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(CaptureCommand::Shutdown);
        if let Some(capture_thread) = self.thread.take() {
            let _ = capture_thread.join();
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{CaptureCommand, CaptureError, mpsc};

    pub fn run(
        _source_id: &str,
        _commands: &mpsc::Receiver<CaptureCommand>,
        ready: &mpsc::SyncSender<Result<(), CaptureError>>,
    ) -> Result<(), CaptureError> {
        let _ = ready.send(Err(CaptureError::UnsupportedPlatform));
        Err(CaptureError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::{
        collections::{HashMap, VecDeque},
        sync::{Arc, mpsc::TryRecvError},
    };

    use windows::{
        Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            Media::MediaFoundation::{
                IMFAttributes, IMFByteStream, IMFMediaSource, IMFSample, IMFSinkWriter,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_AVG_BITRATE,
                MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
                MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
                MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
                MFCreateDeviceSource, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
                MFCreateSinkWriterFromURL, MFCreateSourceReaderFromMediaSource, MFMediaType_Video,
                MFSTARTUP_FULL, MFShutdown, MFStartup, MFVideoFormat_H264, MFVideoFormat_I420,
                MFVideoFormat_IYUV, MFVideoFormat_NV12, MFVideoFormat_YUY2, MFVideoFormat_YV12,
                MFVideoInterlace_Progressive,
            },
            System::{
                Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
                Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
            },
        },
        core::{Error as WindowsError, HSTRING},
    };

    use super::{CaptureCommand, CaptureError, CaptureStats, Path, mpsc};

    const VIDEO_STREAM: u32 =
        u32::from_ne_bytes(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.to_ne_bytes());
    const PRE_ROLL_DURATION_NS: i64 = 2_000_000_000;
    const MAX_PRE_ROLL_FRAMES: usize = 90;
    const OUTPUT_HOLDBACK_DURATION_NS: i64 = 2_000_000_000;
    const MAX_PENDING_OUTPUT_FRAMES: usize = 90;
    const CLOCK_CORRELATION_TOLERANCE_NS: i64 = 10_000_000_000;

    struct PlatformGuard {
        uninitialize_com: bool,
        media_foundation_started: bool,
    }

    impl PlatformGuard {
        fn initialize() -> Result<Self, CaptureError> {
            // SAFETY: COM is initialized and uninitialized on this capture thread only.
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
            // SAFETY: The source was created successfully and is no longer used after this point.
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

    fn choose_input_type(
        reader: &windows::Win32::Media::MediaFoundation::IMFSourceReader,
    ) -> Result<
        (
            windows::Win32::Media::MediaFoundation::IMFMediaType,
            FormatInfo,
        ),
        CaptureError,
    > {
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
            let (width, height) =
                unpacked_pair(unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }?);
            let (rate_num, rate_den) =
                unpacked_pair(unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE) }?);
            if width == 0 || height == 0 || rate_num == 0 || rate_den == 0 {
                continue;
            }
            let pixels = u64::from(width) * u64::from(height);
            let fps_milli = u64::from(rate_num) * 1000 / u64::from(rate_den);
            let within_initial_profile = width <= 1280 && height <= 720 && fps_milli <= 30_000;
            if !within_initial_profile {
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
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, CaptureError> {
        // SAFETY: MFCreateMediaType returns an owned, initially empty media type.
        let output = unsafe { MFCreateMediaType() }?;
        let pixels = u64::from(format.width) * u64::from(format.height);
        let bitrate =
            u32::try_from((pixels * 8).clamp(2_000_000, 16_000_000)).unwrap_or(16_000_000);
        // SAFETY: Values follow the Sink Writer H.264 output-media-type requirements.
        unsafe {
            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
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

    struct ActiveWriter {
        writer: IMFSinkWriter,
        stream: u32,
        first_timestamp: Option<i64>,
        pending_frames: VecDeque<PendingFrame>,
        stats: CaptureStats,
    }

    #[derive(Clone)]
    struct BufferedFrame {
        bytes: Arc<[u8]>,
        source_timestamp: i64,
        monotonic_time_ns: i64,
        duration: Option<i64>,
        flags: Option<u32>,
    }

    struct PendingFrame {
        frame: BufferedFrame,
        from_arm_buffer: bool,
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

    fn create_writer(
        output_path: &Path,
        input_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
        format: FormatInfo,
    ) -> Result<ActiveWriter, CaptureError> {
        let output_type = make_output_type(format)?;
        let output_url = HSTRING::from(output_path.to_string_lossy().as_ref());
        // SAFETY: The URL is a valid local .mp4 path; byte stream and custom attributes are null.
        let writer = unsafe {
            MFCreateSinkWriterFromURL(&output_url, None::<&IMFByteStream>, None::<&IMFAttributes>)
        }?;
        // SAFETY: The media types remain alive through configuration and the writer owns its
        // configured representation after these calls.
        let stream = unsafe { writer.AddStream(&output_type) }?;
        unsafe {
            writer.SetInputMediaType(stream, input_type, None::<&IMFAttributes>)?;
            writer.BeginWriting()?;
        }
        Ok(ActiveWriter {
            writer,
            stream,
            first_timestamp: None,
            pending_frames: VecDeque::new(),
            stats: CaptureStats::default(),
        })
    }

    fn submit_sample(
        active: &mut ActiveWriter,
        sample: &IMFSample,
        source_timestamp: i64,
        monotonic_time_ns: i64,
        from_buffer: bool,
    ) -> Result<(), CaptureError> {
        let origin = *active.first_timestamp.get_or_insert(source_timestamp);
        active
            .stats
            .first_frame_monotonic_time_ns
            .get_or_insert(monotonic_time_ns);
        active.stats.last_frame_monotonic_time_ns = Some(monotonic_time_ns);
        // SAFETY: The sample belongs to this thread. Sink Writer timestamps start at zero.
        unsafe {
            sample.SetSampleTime(source_timestamp.saturating_sub(origin))?;
            active.writer.WriteSample(active.stream, sample)?;
        }
        active.stats.frame_count += 1;
        if from_buffer {
            active.stats.buffered_frame_count += 1;
        }
        Ok(())
    }

    fn copy_buffered_frame(
        sample: &IMFSample,
        source_timestamp: i64,
        monotonic_time_ns: i64,
    ) -> Result<BufferedFrame, CaptureError> {
        // SAFETY: The returned contiguous buffer remains locked only while its bytes are copied.
        let buffer = unsafe { sample.ConvertToContiguousBuffer() }?;
        let mut data = std::ptr::null_mut();
        let mut current_length = 0_u32;
        unsafe { buffer.Lock(&raw mut data, None, Some(&raw mut current_length)) }?;
        let bytes: Arc<[u8]> = if current_length == 0 {
            Arc::from([])
        } else {
            // SAFETY: Lock returned a buffer containing current_length readable bytes.
            unsafe {
                std::slice::from_raw_parts(
                    data.cast_const(),
                    usize::try_from(current_length).expect("u32 always fits in usize on Windows"),
                )
            }
            .into()
        };
        // SAFETY: This balances the successful Lock call above.
        unsafe { buffer.Unlock() }?;
        Ok(BufferedFrame {
            bytes,
            source_timestamp,
            monotonic_time_ns,
            // SAFETY: These getters only read optional attributes from this sample.
            duration: unsafe { sample.GetSampleDuration() }.ok(),
            flags: unsafe { sample.GetSampleFlags() }.ok(),
        })
    }

    fn write_buffered_frame(
        active: &mut ActiveWriter,
        pending: &PendingFrame,
    ) -> Result<(), CaptureError> {
        let frame = &pending.frame;
        let length = u32::try_from(frame.bytes.len()).map_err(|_| CaptureError::FrameTooLarge)?;
        // SAFETY: Both factories return independent objects owned by this function.
        let sample = unsafe { MFCreateSample() }?;
        let buffer = unsafe { MFCreateMemoryBuffer(length) }?;
        let mut destination = std::ptr::null_mut();
        unsafe { buffer.Lock(&raw mut destination, None, None) }?;
        if !frame.bytes.is_empty() {
            // SAFETY: The destination buffer was allocated with exactly frame.bytes.len() bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(frame.bytes.as_ptr(), destination, frame.bytes.len());
            }
        }
        // SAFETY: This balances Lock; length cannot exceed the allocated buffer size.
        unsafe {
            buffer.Unlock()?;
            buffer.SetCurrentLength(length)?;
            sample.AddBuffer(&buffer)?;
            if let Some(duration) = frame.duration {
                sample.SetSampleDuration(duration)?;
            }
            if let Some(flags) = frame.flags {
                sample.SetSampleFlags(flags)?;
            }
        }
        submit_sample(
            active,
            &sample,
            frame.source_timestamp,
            frame.monotonic_time_ns,
            pending.from_arm_buffer,
        )
    }

    fn begin_writer(
        output_path: &Path,
        input_type: &windows::Win32::Media::MediaFoundation::IMFMediaType,
        format: FormatInfo,
        logical_start_monotonic_time_ns: i64,
        buffered_frames: &VecDeque<BufferedFrame>,
    ) -> Result<ActiveWriter, CaptureError> {
        let mut active = create_writer(output_path, input_type, format)?;
        let clock_is_correlated = buffered_frames.back().is_some_and(|frame| {
            frame
                .monotonic_time_ns
                .abs_diff(logical_start_monotonic_time_ns)
                <= u64::try_from(CLOCK_CORRELATION_TOLERANCE_NS)
                    .expect("clock tolerance is positive")
        });
        if clock_is_correlated {
            active.pending_frames.extend(
                buffered_frames
                    .iter()
                    .filter(|frame| frame.monotonic_time_ns >= logical_start_monotonic_time_ns)
                    .cloned()
                    .map(|frame| PendingFrame {
                        frame,
                        from_arm_buffer: true,
                    }),
            );
        }
        Ok(active)
    }

    fn flush_aged_frames(active: &mut ActiveWriter) -> Result<(), CaptureError> {
        loop {
            let should_flush = active.pending_frames.len() > MAX_PENDING_OUTPUT_FRAMES
                || active.pending_frames.front().is_some_and(|oldest| {
                    active.pending_frames.back().is_some_and(|newest| {
                        newest
                            .frame
                            .monotonic_time_ns
                            .saturating_sub(oldest.frame.monotonic_time_ns)
                            > OUTPUT_HOLDBACK_DURATION_NS
                    })
                });
            if !should_flush {
                return Ok(());
            }
            let frame = active
                .pending_frames
                .pop_front()
                .expect("a flushable output queue is not empty");
            write_buffered_frame(active, &frame)?;
        }
    }

    fn finalize_writer(
        mut active: ActiveWriter,
        logical_stop_monotonic_time_ns: Option<i64>,
    ) -> Result<CaptureStats, CaptureError> {
        if let Some(logical_stop) = logical_stop_monotonic_time_ns {
            active.stats.stop_holdback_exceeded = active
                .stats
                .last_frame_monotonic_time_ns
                .is_some_and(|last_written| last_written > logical_stop);
            while active
                .pending_frames
                .front()
                .is_some_and(|pending| pending.frame.monotonic_time_ns <= logical_stop)
            {
                let frame = active
                    .pending_frames
                    .pop_front()
                    .expect("the pending output queue was checked as non-empty");
                write_buffered_frame(&mut active, &frame)?;
            }
            active.stats.discarded_tail_frame_count = u64::try_from(active.pending_frames.len())
                .expect("pending frame count always fits in u64");
            active.pending_frames.clear();
        } else {
            while let Some(frame) = active.pending_frames.pop_front() {
                write_buffered_frame(&mut active, &frame)?;
            }
        }
        tracing::info!(
            frames = active.stats.frame_count,
            discarded_tail_frames = active.stats.discarded_tail_frame_count,
            "finalizing Media Foundation sink writer"
        );
        // SAFETY: BeginWriting succeeded and no further samples are submitted after Finalize.
        unsafe { active.writer.Finalize() }?;
        tracing::info!(
            frames = active.stats.frame_count,
            "Media Foundation sink writer finalized"
        );
        Ok(active.stats)
    }

    pub fn run(
        source_id: &str,
        commands: &mpsc::Receiver<CaptureCommand>,
        ready: &mpsc::SyncSender<Result<(), CaptureError>>,
    ) -> Result<(), CaptureError> {
        let initialized = (|| {
            let platform = PlatformGuard::initialize()?;
            let source = MediaSourceGuard(create_media_source(source_id)?);
            // SAFETY: The source remains alive for the entire reader lifetime; default attributes
            // are sufficient for synchronous capture.
            let reader =
                unsafe { MFCreateSourceReaderFromMediaSource(&source.0, None::<&IMFAttributes>) }?;
            let (input_type, format) = choose_input_type(&reader)?;
            // SAFETY: The selected native type came from this reader and reserved must be null.
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

        let mut active_writers: HashMap<String, ActiveWriter> = HashMap::new();
        let mut failed_writers: HashMap<String, CaptureError> = HashMap::new();
        let mut buffered_frames = VecDeque::new();
        let mut clock_anchor: Option<(i64, i64)> = None;
        'capture: loop {
            loop {
                match commands.try_recv() {
                    Ok(CaptureCommand::Begin {
                        recording_id,
                        output_path,
                        logical_start_monotonic_time_ns,
                        response,
                    }) => {
                        failed_writers.remove(&recording_id);
                        let result = match active_writers.entry(recording_id) {
                            std::collections::hash_map::Entry::Occupied(_) => {
                                Err(CaptureError::AlreadyRecording)
                            }
                            std::collections::hash_map::Entry::Vacant(entry) => begin_writer(
                                &output_path,
                                &input_type,
                                format,
                                logical_start_monotonic_time_ns,
                                &buffered_frames,
                            )
                            .map(|writer| {
                                tracing::info!(
                                    path = %output_path.display(),
                                    buffered_frames = writer.pending_frames.len(),
                                    "prepared capture started recording"
                                );
                                entry.insert(writer);
                            }),
                        };
                        let _ = response.send(result);
                    }
                    Ok(CaptureCommand::End {
                        recording_id,
                        logical_stop_monotonic_time_ns,
                        response,
                    }) => {
                        let result = if let Some(writer) = active_writers.remove(&recording_id) {
                            finalize_writer(writer, Some(logical_stop_monotonic_time_ns))
                        } else if let Some(error) = failed_writers.remove(&recording_id) {
                            Err(error)
                        } else {
                            Err(CaptureError::NotRecording)
                        };
                        let _ = response.send(result);
                    }
                    Ok(CaptureCommand::Abort {
                        recording_id,
                        response,
                    }) => {
                        let result = active_writers
                            .remove(&recording_id)
                            .map_or(Ok(()), |writer| finalize_writer(writer, None).map(drop));
                        failed_writers.remove(&recording_id);
                        let _ = response.send(result);
                    }
                    Ok(CaptureCommand::Shutdown) | Err(TryRecvError::Disconnected) => {
                        for (_, writer) in active_writers.drain() {
                            let _ = finalize_writer(writer, None);
                        }
                        failed_writers.clear();
                        break 'capture;
                    }
                    Err(TryRecvError::Empty) => break,
                }
            }

            let mut flags = 0_u32;
            let mut timestamp = 0_i64;
            let mut sample = None;
            // SAFETY: All optional out-pointers are valid for this synchronous call.
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
            if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                break;
            }
            let Some(sample) = sample else {
                for active in active_writers.values_mut() {
                    active.stats.dropped_frame_count += 1;
                }
                continue;
            };
            let (source_anchor, monotonic_anchor) =
                *clock_anchor.get_or_insert((timestamp, arrival_monotonic_time_ns));
            let source_delta_ns = timestamp.saturating_sub(source_anchor).saturating_mul(100);
            let frame_monotonic_time_ns = monotonic_anchor.saturating_add(source_delta_ns);
            let frame = copy_buffered_frame(&sample, timestamp, frame_monotonic_time_ns)?;
            buffered_frames.push_back(frame.clone());
            while buffered_frames.len() > MAX_PRE_ROLL_FRAMES
                || buffered_frames.front().is_some_and(|oldest| {
                    buffered_frames.back().is_some_and(|newest| {
                        newest
                            .monotonic_time_ns
                            .saturating_sub(oldest.monotonic_time_ns)
                            > PRE_ROLL_DURATION_NS
                    })
                })
            {
                buffered_frames.pop_front();
            }
            let recording_ids: Vec<_> = active_writers.keys().cloned().collect();
            for recording_id in recording_ids {
                let result = {
                    let active = active_writers
                        .get_mut(&recording_id)
                        .expect("recording ID came from the active writer map");
                    active.pending_frames.push_back(PendingFrame {
                        frame: frame.clone(),
                        from_arm_buffer: false,
                    });
                    flush_aged_frames(active)
                };
                if let Err(error) = result {
                    active_writers.remove(&recording_id);
                    tracing::warn!(%recording_id, %error, "isolated recording writer failure");
                    failed_writers.insert(recording_id, error);
                }
            }
        }

        for (_, writer) in active_writers.drain() {
            let _ = finalize_writer(writer, None);
        }
        drop(reader);
        drop(source);
        drop(platform);
        tracing::info!(source_id, "prepared capture stopped");
        Ok(())
    }
}
