// SPDX-License-Identifier: AGPL-3.0-only
// Generated-code-compatible declarations for protocol/daw_video_recorder.proto.
// The .proto file remains the language-neutral source of truth.

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Envelope {
    #[prost(uint32, tag = "1")]
    pub protocol_version: u32,
    #[prost(uint64, tag = "2")]
    pub request_id: u64,
    #[prost(
        oneof = "envelope::Payload",
        tags = "10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 30, 31, 32, 33, 34, 35, 36, 37"
    )]
    pub payload: Option<envelope::Payload>,
}

pub mod envelope {
    #[derive(Clone, PartialEq, ::prost::Oneof)]
    pub enum Payload {
        #[prost(message, tag = "10")]
        Hello(super::Hello),
        #[prost(message, tag = "11")]
        RegisterInstance(super::RegisterInstance),
        #[prost(message, tag = "12")]
        UnregisterInstance(super::UnregisterInstance),
        #[prost(message, tag = "13")]
        ListSources(super::ListSources),
        #[prost(message, tag = "14")]
        SelectSource(super::SelectSource),
        #[prost(message, tag = "15")]
        SetProjectRoot(super::SetProjectRoot),
        #[prost(message, tag = "16")]
        SetArmed(super::SetArmed),
        #[prost(message, tag = "17")]
        TransportSnapshotBatch(super::TransportSnapshotBatch),
        #[prost(message, tag = "18")]
        Heartbeat(super::Heartbeat),
        #[prost(message, tag = "19")]
        ConfigureEncodingProfile(super::ConfigureEncodingProfile),
        #[prost(message, tag = "20")]
        SetSyncOffset(super::SetSyncOffset),
        #[prost(message, tag = "30")]
        HelloAck(super::HelloAck),
        #[prost(message, tag = "31")]
        InstanceRegistered(super::InstanceRegistered),
        #[prost(message, tag = "32")]
        SourceList(super::SourceList),
        #[prost(message, tag = "33")]
        SessionStateChanged(super::SessionStateChanged),
        #[prost(message, tag = "34")]
        RecordingFinalized(super::RecordingFinalized),
        #[prost(message, tag = "35")]
        Error(super::Error),
        #[prost(message, tag = "36")]
        HeartbeatAck(super::HeartbeatAck),
        #[prost(message, tag = "37")]
        TransportAccepted(super::TransportAccepted),
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Hello {
    #[prost(uint32, tag = "1")]
    pub minimum_version: u32,
    #[prost(uint32, tag = "2")]
    pub maximum_version: u32,
    #[prost(string, tag = "3")]
    pub client_name: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct HelloAck {
    #[prost(uint32, tag = "1")]
    pub selected_version: u32,
    #[prost(string, tag = "2")]
    pub service_version: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RegisterInstance {
    #[prost(string, tag = "1")]
    pub persistent_instance_id: String,
    #[prost(string, tag = "2")]
    pub instance_label: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct InstanceRegistered {
    #[prost(string, tag = "1")]
    pub persistent_instance_id: String,
    #[prost(string, tag = "2")]
    pub runtime_instance_id: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct UnregisterInstance {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct ListSources {}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VideoFormat {
    #[prost(uint32, tag = "1")]
    pub width: u32,
    #[prost(uint32, tag = "2")]
    pub height: u32,
    #[prost(uint32, tag = "3")]
    pub frames_per_second_numerator: u32,
    #[prost(uint32, tag = "4")]
    pub frames_per_second_denominator: u32,
    #[prost(string, tag = "5")]
    pub pixel_format: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VideoSource {
    #[prost(string, tag = "1")]
    pub source_id: String,
    #[prost(string, tag = "2")]
    pub display_name: String,
    #[prost(message, repeated, tag = "3")]
    pub formats: Vec<VideoFormat>,
    #[prost(bool, tag = "4")]
    pub available: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SourceList {
    #[prost(message, repeated, tag = "1")]
    pub sources: Vec<VideoSource>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SelectSource {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(string, tag = "2")]
    pub source_id: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SetProjectRoot {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(string, tag = "2")]
    pub project_root: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SetArmed {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(bool, tag = "2")]
    pub armed: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConfigureEncodingProfile {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(uint32, tag = "2")]
    pub bit_rate: u32,
    #[prost(uint32, tag = "3")]
    pub gop_duration_ms: u32,
    #[prost(bool, tag = "4")]
    pub prefer_hardware: bool,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SetSyncOffset {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(int64, tag = "2")]
    pub offset_ns: i64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransportSnapshot {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(uint64, tag = "2")]
    pub sequence_number: u64,
    #[prost(int64, tag = "3")]
    pub callback_monotonic_time_ns: i64,
    #[prost(int64, optional, tag = "4")]
    pub host_time_ns: Option<i64>,
    #[prost(int64, optional, tag = "5")]
    pub project_sample_position: Option<i64>,
    #[prost(double, tag = "6")]
    pub sample_rate: f64,
    #[prost(bool, tag = "7")]
    pub is_playing: bool,
    #[prost(bool, tag = "8")]
    pub is_recording: bool,
    #[prost(bool, tag = "9")]
    pub is_looping: bool,
    #[prost(bool, tag = "10")]
    pub is_offline_rendering: bool,
    #[prost(double, optional, tag = "11")]
    pub tempo: Option<f64>,
    #[prost(int32, optional, tag = "12")]
    pub time_signature_numerator: Option<i32>,
    #[prost(int32, optional, tag = "13")]
    pub time_signature_denominator: Option<i32>,
    #[prost(double, optional, tag = "14")]
    pub ppq_position: Option<f64>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransportSnapshotBatch {
    #[prost(message, repeated, tag = "1")]
    pub snapshots: Vec<TransportSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum SessionState {
    Unspecified = 0,
    Idle = 1,
    Preparing = 2,
    Armed = 3,
    Recording = 4,
    Finalizing = 5,
    Error = 6,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SessionStateChanged {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(enumeration = "SessionState", tag = "2")]
    pub state: i32,
    #[prost(string, tag = "3")]
    pub detail: String,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct RecordingFinalized {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(string, tag = "2")]
    pub take_id: String,
    #[prost(string, tag = "3")]
    pub video_path: String,
    #[prost(string, tag = "4")]
    pub metadata_path: String,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct Heartbeat {
    #[prost(uint64, tag = "1")]
    pub sequence_number: u64,
}

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct HeartbeatAck {
    #[prost(uint64, tag = "1")]
    pub sequence_number: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransportAccepted {
    #[prost(string, tag = "1")]
    pub runtime_instance_id: String,
    #[prost(uint64, tag = "2")]
    pub last_sequence_number: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Error {
    #[prost(string, tag = "1")]
    pub code: String,
    #[prost(string, tag = "2")]
    pub message: String,
    #[prost(string, tag = "3")]
    pub runtime_instance_id: String,
    #[prost(string, tag = "4")]
    pub source_id: String,
    #[prost(bool, tag = "5")]
    pub recoverable: bool,
}
