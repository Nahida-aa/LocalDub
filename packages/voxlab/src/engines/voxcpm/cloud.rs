//! Rust port of the `VoxCPMCloud` TTS backend (packages/voxlab/src/engines/voxcpm/cloud.ts).
//!
//! Talks to a Gradio 5 `/generate` endpoint (e.g. https://voxcpm.modelbest.cn) over the
//! two-request Gradio API protocol:
//!   1. POST  /gradio_api/call/generate  -> {"event_id": "..."}
//!   2. GET   /gradio_api/call/generate/{event_id}  (SSE stream)
//!      `event: complete` carries `data: [ {url|path}, null ]` (the synthesized WAV).

use serde::Deserialize;
use std::io::BufRead;
use std::time::Duration;

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
    base_url: String,
    control_instruction: String,
    client: reqwest::blocking::Client,
}

/// Result of a single TTS generation: raw PCM f32 samples normalized to [`TARGET_SAMPLE_RATE`].
#[derive(Debug)]
pub struct TtsResult {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub gen_time_sec: f64,
}

impl VoxCPMCloud {
    pub fn new(config: VoxCPMCloudConfig) -> Self {
        let base_url = config
            .api_url
            .unwrap_or_else(|| DEFAULT_API_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .expect("failed to build reqwest client");
        Self {
            base_url,
            control_instruction: config.control_instruction.unwrap_or_default(),
            client,
        }
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

        // Upload the local reference audio via Gradio's multipart /gradio_api/upload endpoint,
        // then reference it by the server-side path/url in the predict payload. (Inline base64
        // FileData is rejected by this server, so we must upload first — mirrors how
        // @gradio/client's handle_file() works.)
        let server_path = self.upload_file(reference_wav_path)?;
        let server_url = format!("{}/gradio_api/file={}", self.base_url, server_path);

        let payload = serde_json::json!({
            "data": [
                text,
                self.control_instruction,
                { "path": server_path, "url": server_url, "meta": { "_type": "gradio.FileData" } },
                false,                       // use_prompt_text / is_ultimate
                prompt_text.unwrap_or(""),   // prompt_text_value
                cfg_value,                   // cfg_value
                false,                       // do_normalize
                false,                       // denoise / ref_denoise
                10,                          // dit_steps
                ""                           // user_id
            ]
        });

        // 1) POST -> event_id
        let post_url = format!("{}/gradio_api/call/generate", self.base_url);
        let resp = self
            .client
            .post(&post_url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .map_err(|e| anyhow::anyhow!("POST generate failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "POST generate returned status {}",
                resp.status()
            ));
        }
        let event_id: EventIdResp = resp
            .json()
            .map_err(|e| anyhow::anyhow!("parse event_id response: {e}"))?;
        let event_id = event_id.event_id;

        // 2) GET SSE stream -> audio url/path
        let get_url = format!("{}/gradio_api/call/generate/{}", self.base_url, event_id);
        let stream = self
            .client
            .get(&get_url)
            .send()
            .map_err(|e| anyhow::anyhow!("GET generate stream failed: {e}"))?;
        if !stream.status().is_success() {
            return Err(anyhow::anyhow!(
                "GET generate stream returned status {}",
                stream.status()
            ));
        }

        let audio_loc = parse_sse_stream(stream)?;

        // 3) Download the produced WAV
        let audio_url = audio_loc
            .url
            .filter(|u| !u.is_empty())
            .or_else(|| audio_loc.path.filter(|p| !p.is_empty()))
            .ok_or_else(|| anyhow::anyhow!("generate returned no audio url/path"))?;
        let wav_bytes = self
            .client
            .get(&audio_url)
            .send()
            .map_err(|e| anyhow::anyhow!("download audio failed: {e}"))?
            .bytes()
            .map_err(|e| anyhow::anyhow!("read audio bytes: {e}"))?;

        let (samples, sample_rate) = parse_wav(&wav_bytes)?;
        let (samples, sample_rate) = if sample_rate != TARGET_SAMPLE_RATE {
            (
                resample(&samples, sample_rate, TARGET_SAMPLE_RATE),
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

    /// Upload a local file to the Gradio server's `/gradio_api/upload` endpoint (multipart
    /// form field `files`), returning the server-side path (e.g. `/tmp/gradio/.../x.wav`).
    fn upload_file(&self, local_path: &str) -> anyhow::Result<String> {
        let upload_url = format!("{}/gradio_api/upload", self.base_url);
        let file_bytes = std::fs::read(local_path)
            .map_err(|e| anyhow::anyhow!("read upload file {local_path}: {e}"))?;
        let part = reqwest::blocking::multipart::Part::bytes(file_bytes).file_name("reference.wav");
        let form = reqwest::blocking::multipart::Form::new().part("files", part);

        let resp = self
            .client
            .post(&upload_url)
            .multipart(form)
            .send()
            .map_err(|e| anyhow::anyhow!("upload failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("upload returned status {}", resp.status()));
        }
        let paths: Vec<String> = resp
            .json()
            .map_err(|e| anyhow::anyhow!("parse upload response: {e}"))?;
        paths
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("upload response contained no path"))
    }
}

#[derive(Deserialize)]
struct EventIdResp {
    event_id: String,
}

/// A single FileData element in the SSE `complete` payload.
#[derive(Deserialize, Default)]
struct AudioFile {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

/// Parse the Gradio SSE stream and extract the audio FileData from the `event: complete` event.
fn parse_sse_stream(stream: reqwest::blocking::Response) -> anyhow::Result<AudioFile> {
    let reader = std::io::BufReader::new(stream);
    let mut event = String::new();
    let mut data_lines: Vec<String> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if line.is_empty() {
            // End of an event block; dispatch.
            if event == "complete" {
                let joined = data_lines.concat();
                // The complete payload is a JSON array whose first element is the audio
                // FileData and the rest may be null (e.g. `[ {url,path,...}, null ]`).
                let arr: Vec<Option<AudioFile>> = serde_json::from_str(&joined)
                    .map_err(|e| anyhow::anyhow!("parse complete data: {e} (raw: {joined})"))?;
                if let Some(Some(first)) = arr.into_iter().next() {
                    return Ok(first);
                }
                return Err(anyhow::anyhow!("complete event had no audio FileData"));
            } else if event == "error" {
                let joined = data_lines.concat();
                return Err(anyhow::anyhow!("gradio error event: {joined}"));
            }
            event.clear();
            data_lines.clear();
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim().to_string());
        }
        // ignore other lines (e.g. `id:`, `retry:`)
    }

    Err(anyhow::anyhow!(
        "SSE stream ended without a complete/error event"
    ))
}

/// Parse a RIFF/WAV buffer into mono f32 samples in [-1, 1] and its sample rate.
/// Supports 16-bit and 32-bit PCM. Falls back to treating the whole buffer as raw 16-bit PCM.
fn parse_wav(buf: &[u8]) -> anyhow::Result<(Vec<f32>, u32)> {
    let mut sample_rate = TARGET_SAMPLE_RATE;
    let mut offset = 12usize;
    let len = buf.len();

    let read_str = |buf: &[u8], pos: usize, n: usize| -> String {
        String::from_utf8_lossy(&buf[pos..pos + n]).to_string()
    };
    let rd_u32 = |buf: &[u8], pos: usize| -> u32 {
        u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]])
    };

    while offset + 8 <= len {
        let chunk_id = read_str(buf, offset, 4);
        let chunk_size = rd_u32(buf, offset + 4) as usize;
        if chunk_id == "fmt " {
            sample_rate = rd_u32(buf, offset + 12);
        } else if chunk_id == "data" {
            let data_start = offset + 8;
            let pcm_count = (chunk_size.min(len - data_start)) / 2;
            if data_start + pcm_count * 2 > len {
                break;
            }
            let mut samples = Vec::with_capacity(pcm_count);
            for i in 0..pcm_count {
                let pos = data_start + i * 2;
                let v = i16::from_le_bytes([buf[pos], buf[pos + 1]]);
                samples.push(v as f32 / 32768.0);
            }
            return Ok((samples, sample_rate));
        }
        offset += 8 + chunk_size + (chunk_size & 1);
    }

    // No data chunk: treat whole buffer as raw 16-bit PCM.
    let pcm_count = len / 2;
    let mut samples = Vec::with_capacity(pcm_count);
    for i in 0..pcm_count {
        let pos = i * 2;
        let v = i16::from_le_bytes([buf[pos], buf[pos + 1]]);
        samples.push(v as f32 / 32768.0);
    }
    Ok((samples, sample_rate))
}

/// Linear-interpolation resampler (matches TS `resample`).
fn resample(src: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return src.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let len = (src.len() as f64 * ratio).ceil() as usize;
    let mut dst = Vec::with_capacity(len);
    for i in 0..len {
        let pos = i as f64 / ratio;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let s = if idx + 1 < src.len() {
            src[idx] * (1.0 - frac as f32) + src[idx + 1] * frac as f32
        } else {
            src[idx.min(src.len() - 1)]
        };
        dst.push(s);
    }
    dst
}

/// Write mono f32 samples to a 16-bit PCM WAV file, peak-normalized to 0.95.
pub fn write_wav(samples: &[f32], sample_rate: u32, path: &str) -> anyhow::Result<()> {
    let peak = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max)
        .max(1e-6);
    let scale = 0.95 * 32768.0 / peak;

    let n = samples.len();
    let mut buf = vec![0u8; 44 + n * 2];
    buf[0..4].copy_from_slice(b"RIFF");
    let chunk_size = 36u32 + (n * 2) as u32;
    buf[4..8].copy_from_slice(&chunk_size.to_le_bytes());
    buf[8..12].copy_from_slice(b"WAVE");
    buf[12..16].copy_from_slice(b"fmt ");
    buf[16..20].copy_from_slice(&16u32.to_le_bytes());
    buf[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    buf[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    buf[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    buf[28..32].copy_from_slice(&(sample_rate * 2).to_le_bytes());
    buf[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    buf[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits
    buf[36..40].copy_from_slice(b"data");
    buf[40..44].copy_from_slice(&((n * 2) as u32).to_le_bytes());
    for (i, &s) in samples.iter().enumerate() {
        let v = (s * scale).clamp(-32768.0, 32767.0).round() as i16;
        let pos = 44 + i * 2;
        buf[pos..pos + 2].copy_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, &buf).map_err(|e| anyhow::anyhow!("write wav {path}: {e}"))?;
    Ok(())
}
