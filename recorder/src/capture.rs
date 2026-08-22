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
    #[cfg(not(windows))]
    #[error("camera capture is currently supported on Windows only")]
    UnsupportedPlatform,
}

#[derive(Debug)]
pub struct CaptureHandle {
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<Result<CaptureStats, CaptureError>>>,
}

impl CaptureHandle {
    pub fn start(source_id: String, output_path: PathBuf) -> Result<Self, CaptureError> {
        let (stop_sender, stop_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let capture_thread = thread::Builder::new()
            .name("DAW Video Recorder Capture".into())
            .spawn(move || platform::run(&source_id, &output_path, &stop_receiver, &ready_sender))
            .map_err(|_| CaptureError::ThreadPanicked)?;

        match ready_receiver.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                stop: stop_sender,
                thread: Some(capture_thread),
            }),
            Ok(Err(error)) => {
                let _ = capture_thread.join();
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = stop_sender.send(());
                let _ = capture_thread.join();
                Err(CaptureError::InitializationTimeout)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = capture_thread.join();
                Err(CaptureError::InitializationDisconnected)
            }
        }
    }

    pub fn stop(mut self) -> Result<CaptureStats, CaptureError> {
        let _ = self.stop.send(());
        self.thread
            .take()
            .expect("capture thread is present until stop")
            .join()
            .map_err(|_| CaptureError::ThreadPanicked)?
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(capture_thread) = self.thread.take() {
            let _ = capture_thread.join();
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{CaptureError, CaptureStats, Path, mpsc};

    pub fn run(
        _source_id: &str,
        _output_path: &Path,
        _stop: &mpsc::Receiver<()>,
        ready: &mpsc::SyncSender<Result<(), CaptureError>>,
    ) -> Result<CaptureStats, CaptureError> {
        let _ = ready.send(Err(CaptureError::UnsupportedPlatform));
        Err(CaptureError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::sync::mpsc::TryRecvError;

    use windows::{
        Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            Media::MediaFoundation::{
                IMFAttributes, IMFByteStream, IMFMediaSource, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_AVG_BITRATE,
                MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
                MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
                MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFCreateAttributes,
                MFCreateDeviceSource, MFCreateMediaType, MFCreateSinkWriterFromURL,
                MFCreateSourceReaderFromMediaSource, MFMediaType_Video, MFSTARTUP_FULL, MFShutdown,
                MFStartup, MFVideoFormat_H264, MFVideoFormat_I420, MFVideoFormat_IYUV,
                MFVideoFormat_NV12, MFVideoFormat_YUY2, MFVideoFormat_YV12,
                MFVideoInterlace_Progressive,
            },
            System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        },
        core::{Error as WindowsError, HSTRING},
    };

    use super::{CaptureError, CaptureStats, Path, mpsc};

    const VIDEO_STREAM: u32 =
        u32::from_ne_bytes(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0.to_ne_bytes());

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

    pub fn run(
        source_id: &str,
        output_path: &Path,
        stop: &mpsc::Receiver<()>,
        ready: &mpsc::SyncSender<Result<(), CaptureError>>,
    ) -> Result<CaptureStats, CaptureError> {
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

            let output_type = make_output_type(format)?;
            let output_url = HSTRING::from(output_path.to_string_lossy().as_ref());
            // SAFETY: The URL is a valid local .mp4 path; byte stream and custom attributes are null.
            let writer = unsafe {
                MFCreateSinkWriterFromURL(
                    &output_url,
                    None::<&IMFByteStream>,
                    None::<&IMFAttributes>,
                )
            }?;
            // SAFETY: The media types remain alive through configuration and the writer owns its
            // configured representation after these calls.
            let writer_stream = unsafe { writer.AddStream(&output_type) }?;
            unsafe {
                writer.SetInputMediaType(writer_stream, &input_type, None::<&IMFAttributes>)?;
                writer.BeginWriting()?;
            }
            Ok((platform, source, reader, writer, writer_stream))
        })();

        let (platform, source, reader, writer, writer_stream) = match initialized {
            Ok(state) => {
                tracing::info!(path = %output_path.display(), "capture pipeline initialized");
                let _ = ready.send(Ok(()));
                state
            }
            Err(error) => {
                let _ = ready.send(Err(error));
                return Ok(CaptureStats::default());
            }
        };

        let mut stats = CaptureStats::default();
        let mut first_timestamp = None;
        loop {
            match stop.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => {
                    tracing::info!(frames = stats.frame_count, "capture stop requested");
                    break;
                }
                Err(TryRecvError::Empty) => {}
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
            if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
                break;
            }
            let Some(sample) = sample else {
                stats.dropped_frame_count += 1;
                continue;
            };
            let origin = *first_timestamp.get_or_insert(timestamp);
            // SAFETY: The sample belongs to this thread. Sink Writer timestamps start at zero.
            unsafe {
                sample.SetSampleTime(timestamp.saturating_sub(origin))?;
                writer.WriteSample(writer_stream, &sample)?;
            }
            stats.frame_count += 1;
        }

        // SAFETY: BeginWriting succeeded and no further samples are submitted after Finalize.
        tracing::info!(
            frames = stats.frame_count,
            "finalizing Media Foundation sink writer"
        );
        unsafe { writer.Finalize() }?;
        tracing::info!(
            frames = stats.frame_count,
            "Media Foundation sink writer finalized"
        );
        drop(writer);
        drop(reader);
        drop(source);
        drop(platform);
        Ok(stats)
    }
}
