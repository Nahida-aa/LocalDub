//! VoxLab — VoxCPM TTS library (Rust side).
//!
//! Currently provides the cloud (Gradio) TTS backend, ported from the TypeScript
//! implementation in `packages/voxlab/src/engines/voxcpm/cloud.ts`.

pub mod engines;
pub mod gradio_client;
pub mod wav;

pub use engines::voxcpm::cloud::{
    DEFAULT_API_URL, TARGET_SAMPLE_RATE, TtsResult, VoxCPMCloud, VoxCPMCloudConfig,
};
pub use gradio_client::GradioClient;
pub use wav::{read_wav, resample, write_wav};
