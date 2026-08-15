//! Rust port of the `VoxCPMCloud` TTS backend (packages/voxlab/src/engines/voxcpm/cloud.ts).
//!
//! Uses the in-crate [`crate::gradio_client::GradioClient`] to talk to a Gradio 5
//! `/generate` endpoint (e.g. https://voxcpm.modelbest.cn).

use serde_json::Value;
use serde_json::json;

use crate::gradio_client::GradioClient;
use crate::wav;

pub const DEFAULT_API_URL: &str = "https://voxcpm.modelbest.cn";

/// Output sample rate we normalize everything to (matches TS cloud.ts).
pub const TARGET_SAMPLE_RATE: u32 = 48000;

#[derive(Debug, Clone, Default)]
pub struct VoxCPMCloudConfig {
    /// Base URL of the Gradio server (no trailing slash). Defaults to [`DEFAULT_API_URL`].
    pub api_url: Option<String>,
    /// Optional control instruction for text-only synthesis.
    pub control_instruction: Option<String>,
}

#[derive(Debug)]
pub struct VoxCPMCloud {
    client: GradioClient,
    control_instruction: String,
}

/// Result of a single TTS generation: raw PCM f32 samples normalized to [`TARGET_SAMPLE_RATE`].
#[derive(Debug)]
pub struct TtsResult {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub gen_time_sec: f64,
}

impl VoxCPMCloud {
    pub fn new(config: VoxCPMCloudConfig) -> anyhow::Result<Self> {
        let base_url = config
            .api_url
            .unwrap_or_else(|| DEFAULT_API_URL.to_string());
        Ok(Self {
            client: GradioClient::connect(&base_url)?,
            control_instruction: config.control_instruction.unwrap_or_default(),
        })
    }

    /// Generate speech from `text` using a local reference WAV file (`reference_wav_path`).
    /// `prompt_text` is the original-language transcription of the reference audio (improves
    /// cloning quality); pass empty/None when unavailable.
    pub fn generate(
        &self,
        text: &str,
        reference_wav_path: &str,
        prompt_text: Option<&str>,
        cfg_value: f64,
    ) -> anyhow::Result<TtsResult> {
        let t_start = std::time::Instant::now();

        // Upload the local reference audio, then reference it by server path/url.
        let ref_file = self.client.handle_file(reference_wav_path)?;

        let data: Vec<Value> = vec![
            json!(text),
            json!(self.control_instruction),
            json!({ "path": ref_file.path, "url": ref_file.url, "meta": { "_type": "gradio.FileData" } }),
            json!(false),                     // use_prompt_text / is_ultimate
            json!(prompt_text.unwrap_or("")), // prompt_text_value
            json!(cfg_value),                 // cfg_value
            json!(false),                     // do_normalize
            json!(false),                     // denoise / ref_denoise
            json!(10),                        // dit_steps
            json!(""),                        // user_id
        ];

        let result = self.client.predict("/generate", data)?;

        // The complete payload's first element is the audio FileData; the rest may be null.
        let audio = result
            .into_iter()
            .find_map(|v| v.as_object().map(|o| o.clone()))
            .ok_or_else(|| anyhow::anyhow!("generate returned no audio FileData"))?;
        let audio_url = audio
            .get("url")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                audio
                    .get("path")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
            })
            .ok_or_else(|| anyhow::anyhow!("generate returned no audio url/path"))?
            .to_string();

        let wav_bytes = self.client.download(&audio_url)?;
        let (samples, sample_rate) = wav::read_wav(&wav_bytes)?;
        let (samples, sample_rate) = if sample_rate != TARGET_SAMPLE_RATE {
            (
                wav::resample(&samples, sample_rate, TARGET_SAMPLE_RATE),
                TARGET_SAMPLE_RATE,
            )
        } else {
            (samples, sample_rate)
        };

        Ok(TtsResult {
            samples,
            sample_rate,
            gen_time_sec: t_start.elapsed().as_secs_f64(),
        })
    }
}
