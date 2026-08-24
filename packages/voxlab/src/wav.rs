//! Minimal WAV read/write + resampling (kept dependency-free).
//!
//! Mirrors `packages/voxlab/src/wav.ts`. Reads 16/32-bit PCM, writes 16-bit PCM
//! peak-normalized to 0.95.

use anyhow::Result;

/// Parse a RIFF/WAV buffer into mono f32 samples in [-1, 1] and its sample rate.
/// Supports 16-bit and 32-bit PCM. Falls back to treating the whole buffer as raw
/// 16-bit PCM if no `data` chunk is found.
pub fn read_wav(buf: &[u8]) -> Result<(Vec<f32>, u32)> {
    let mut sample_rate = 48000u32;
    let mut offset = 12usize;
    let len = buf.len();

    while offset + 8 <= len {
        let chunk_id = String::from_utf8_lossy(&buf[offset..offset + 4]).to_string();
        let chunk_size = u32::from_le_bytes([
            buf[offset + 4],
            buf[offset + 5],
            buf[offset + 6],
            buf[offset + 7],
        ]) as usize;
        if chunk_id == "fmt " {
            sample_rate = u32::from_le_bytes([
                buf[offset + 12],
                buf[offset + 13],
                buf[offset + 14],
                buf[offset + 15],
            ]);
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
pub fn resample(src: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
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
pub fn write_wav(samples: &[f32], sample_rate: u32, path: &str) -> Result<()> {
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
