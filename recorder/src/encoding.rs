// SPDX-License-Identifier: AGPL-3.0-only

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_BIT_RATE: u32 = 8_000_000;
pub const MIN_BIT_RATE: u32 = 500_000;
pub const MAX_BIT_RATE: u32 = 50_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EncodingProfile {
    pub bit_rate: u32,
    pub gop_duration_ms: u32,
    pub prefer_hardware: bool,
}

impl Default for EncodingProfile {
    fn default() -> Self {
        Self {
            bit_rate: DEFAULT_BIT_RATE,
            gop_duration_ms: 2_000,
            prefer_hardware: true,
        }
    }
}

impl EncodingProfile {
    pub fn validate(self) -> Result<Self, EncodingProfileError> {
        if !(MIN_BIT_RATE..=MAX_BIT_RATE).contains(&self.bit_rate) {
            return Err(EncodingProfileError::BitRate(self.bit_rate));
        }
        if !(250..=10_000).contains(&self.gop_duration_ms) {
            return Err(EncodingProfileError::GopDuration(self.gop_duration_ms));
        }
        Ok(self)
    }

    #[must_use]
    pub fn stable_id(self) -> String {
        format!(
            "h264-{}bps-gop{}ms-{}",
            self.bit_rate,
            self.gop_duration_ms,
            if self.prefer_hardware {
                "hardware-preferred"
            } else {
                "software"
            }
        )
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EncodingProfileError {
    #[error("encoding bit rate must be between {MIN_BIT_RATE} and {MAX_BIT_RATE} bps: {0}")]
    BitRate(u32),
    #[error("GOP duration must be between 250 and 10000 ms: {0}")]
    GopDuration(u32),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncodingBackend {
    Hardware,
    #[default]
    Software,
}

impl EncodingBackend {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hardware => "media-foundation-hardware-mft",
            Self::Software => "media-foundation-software-mft",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockDrift {
    pub duration_ns: i64,
    pub drift_ns: i64,
    pub drift_ppm: f64,
}

#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn measure_clock_drift(
    start_project_sample: Option<i64>,
    end_project_sample: Option<i64>,
    sample_rate: f64,
    first_frame_monotonic_time_ns: Option<i64>,
    last_frame_monotonic_time_ns: Option<i64>,
) -> Option<ClockDrift> {
    let project_samples = end_project_sample?.checked_sub(start_project_sample?)?;
    let video_duration =
        last_frame_monotonic_time_ns?.checked_sub(first_frame_monotonic_time_ns?)?;
    if project_samples <= 0 || video_duration <= 0 || !sample_rate.is_finite() || sample_rate <= 0.0
    {
        return None;
    }
    let daw_duration = ((project_samples as f64 / sample_rate) * 1_000_000_000.0).round();
    if !(1.0..=(i64::MAX as f64)).contains(&daw_duration) {
        return None;
    }
    let duration_ns = daw_duration as i64;
    let drift_ns = video_duration.saturating_sub(duration_ns);
    Some(ClockDrift {
        duration_ns,
        drift_ns,
        drift_ppm: drift_ns as f64 * 1_000_000.0 / duration_ns as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_profile_bounds_and_produces_stable_id() {
        let profile = EncodingProfile::default().validate().unwrap();
        assert_eq!(
            profile.stable_id(),
            "h264-8000000bps-gop2000ms-hardware-preferred"
        );
        assert!(matches!(
            EncodingProfile {
                bit_rate: 1,
                ..profile
            }
            .validate(),
            Err(EncodingProfileError::BitRate(1))
        ));
    }

    #[test]
    fn measures_thirty_minute_clock_drift_without_waiting_in_real_time() {
        let sample_rate = 48_000.0;
        let duration_seconds = 30_i64 * 60;
        let expected_ns = duration_seconds * 1_000_000_000_i64;
        let simulated_drift_ns = 18_000_000_i64;
        let drift = measure_clock_drift(
            Some(0),
            Some(duration_seconds * 48_000),
            sample_rate,
            Some(1_000_000_000),
            Some(1_000_000_000 + expected_ns + simulated_drift_ns),
        )
        .unwrap();

        assert_eq!(drift.duration_ns, expected_ns);
        assert_eq!(drift.drift_ns, simulated_drift_ns);
        assert!((drift.drift_ppm - 10.0).abs() < f64::EPSILON);
    }
}
