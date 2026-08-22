// SPDX-License-Identifier: AGPL-3.0-only

use thiserror::Error;

use crate::protocol::VideoSource;

#[derive(Debug, Error)]
pub enum SourceError {
    #[cfg(windows)]
    #[error("Media Foundation camera enumeration failed: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("Media Foundation returned an invalid camera device")]
    InvalidDevice,
    #[cfg(not(windows))]
    #[error("camera enumeration is currently supported on Windows only")]
    UnsupportedPlatform,
}

/// Enumerates the video-capture devices currently exposed by the operating system.
pub fn enumerate_video_sources() -> Result<Vec<VideoSource>, SourceError> {
    platform::enumerate_video_sources()
}

#[cfg(not(windows))]
mod platform {
    use super::{SourceError, VideoSource};

    pub fn enumerate_video_sources() -> Result<Vec<VideoSource>, SourceError> {
        Err(SourceError::UnsupportedPlatform)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::{ffi::c_void, ptr, slice};

    use windows::{
        Win32::{
            Foundation::RPC_E_CHANGED_MODE,
            Media::MediaFoundation::{
                IMFActivate, IMFAttributes, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_VERSION,
                MFCreateAttributes, MFEnumDeviceSources, MFSTARTUP_LITE, MFShutdown, MFStartup,
            },
            System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
        },
        core::{Error as WindowsError, HRESULT, PWSTR},
    };

    use super::{SourceError, VideoSource};

    struct ComGuard {
        uninitialize: bool,
    }

    impl ComGuard {
        fn initialize() -> Result<Self, SourceError> {
            // SAFETY: A null reserved pointer is required by CoInitializeEx. This guard remains on
            // the same thread and balances successful initialization with CoUninitialize.
            let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if result.is_ok() {
                Ok(Self { uninitialize: true })
            } else if result == RPC_E_CHANGED_MODE {
                // The caller's thread already selected another apartment model. COM is usable, but
                // this call did not acquire a reference and therefore must not be uninitialized.
                Ok(Self {
                    uninitialize: false,
                })
            } else {
                Err(WindowsError::from_hresult(result).into())
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.uninitialize {
                // SAFETY: This exactly balances the successful CoInitializeEx call above.
                unsafe { CoUninitialize() };
            }
        }
    }

    struct MediaFoundationGuard;

    impl MediaFoundationGuard {
        fn initialize() -> Result<Self, SourceError> {
            // SAFETY: MFStartup is called from a non-work-queue service thread and is balanced by
            // the Drop implementation below.
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }?;
            Ok(Self)
        }
    }

    impl Drop for MediaFoundationGuard {
        fn drop(&mut self) {
            // SAFETY: This guard is constructed only after MFStartup succeeds.
            let _ = unsafe { MFShutdown() };
        }
    }

    struct TaskMem(*const c_void);

    impl Drop for TaskMem {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: Media Foundation allocates this memory with the COM task allocator.
                unsafe { CoTaskMemFree(Some(self.0)) };
            }
        }
    }

    fn allocated_string(
        attributes: &IMFAttributes,
        key: *const windows::core::GUID,
    ) -> Result<String, SourceError> {
        let mut value = PWSTR::null();
        let mut length = 0_u32;
        // SAFETY: Both output pointers are valid for the duration of the call. The returned buffer
        // is copied immediately and released by TaskMem.
        unsafe { attributes.GetAllocatedString(key, &raw mut value, &raw mut length) }?;
        let allocation = TaskMem(value.0.cast());
        if value.is_null() {
            return Ok(String::new());
        }
        // SAFETY: GetAllocatedString reports the number of UTF-16 code units excluding the null
        // terminator, and the allocation remains alive until the end of this function.
        let units = unsafe { slice::from_raw_parts(value.0, length as usize) };
        let result = String::from_utf16_lossy(units);
        drop(allocation);
        Ok(result)
    }

    fn take_activations(devices: *mut Option<IMFActivate>, count: u32) -> Vec<Option<IMFActivate>> {
        if devices.is_null() || count == 0 {
            return Vec::new();
        }
        let mut result = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            // SAFETY: MFEnumDeviceSources returned an array with count initialized entries. Moving
            // each COM wrapper into the Vec transfers ownership before freeing only the array.
            result.push(unsafe { ptr::read(devices.add(index)) });
        }
        result
    }

    pub fn enumerate_video_sources() -> Result<Vec<VideoSource>, SourceError> {
        let _com = ComGuard::initialize()?;
        let _media_foundation = MediaFoundationGuard::initialize()?;

        let mut attributes = None;
        // SAFETY: The output pointer is valid and receives an owned IMFAttributes wrapper.
        unsafe { MFCreateAttributes(&raw mut attributes, 1) }?;
        let attributes = attributes.ok_or(SourceError::InvalidDevice)?;
        // SAFETY: Both GUID pointers refer to static constants for the duration of the call.
        unsafe {
            attributes.SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
        }?;

        let mut devices = ptr::null_mut();
        let mut count = 0_u32;
        // SAFETY: Media Foundation receives valid out-pointers. The returned COM wrappers and the
        // containing task-allocated array are released below.
        unsafe { MFEnumDeviceSources(&attributes, &raw mut devices, &raw mut count) }?;
        let allocation = TaskMem(devices.cast());
        let activations = take_activations(devices, count);
        drop(allocation);

        let mut sources = Vec::with_capacity(activations.len());
        for activation in activations {
            let activation = activation.ok_or(SourceError::InvalidDevice)?;
            let display_name =
                allocated_string(&activation, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)?;
            let source_id = allocated_string(
                &activation,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            )?;
            if source_id.is_empty() {
                continue;
            }
            sources.push(VideoSource {
                source_id,
                display_name,
                formats: Vec::new(),
                available: true,
            });
        }
        sources.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        Ok(sources)
    }

    const _: HRESULT = RPC_E_CHANGED_MODE;
}
