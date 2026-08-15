//! VoxLab — VoxCPM TTS library (Rust side).
//!
//! Currently provides the cloud (Gradio) TTS backend, ported from the TypeScript
//! implementation in `packages/voxlab/src/engines/voxcpm/cloud.ts`.

pub mod engines;

pub use engines::voxcpm::cloud::{
    DEFAULT_API_URL, TARGET_SAMPLE_RATE, TtsResult, VoxCPMCloud, VoxCPMCloudConfig, write_wav,
};
