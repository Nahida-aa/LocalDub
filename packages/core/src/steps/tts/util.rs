//! tts 步骤的辅助逻辑：不直接参与编排、可独立测试的小函数。
//!
//! 从 `mod.rs` 拆出，让 `step_tts` 只保留编排主流程。

use crate::steps::tts::args::{TtsDevice, TtsRuntime};
use crate::steps::tts::out::TtsFile;
use crate::steps::utils::find_release_bin;

/// TTS 静音段统一采样率。VoxCPM cloud 归一化到 48000Hz (`voxcpm_cloud::TARGET_SAMPLE_RATE`),
/// 本地二进制输出也为 48k, 故静音占位统一用 48000Hz 以保证与合成段、mix_audio 探测一致。
const SILENT_SAMPLE_RATE: u32 = 48000;

/// 写一段合法的静音 wav (47000Hz 单声道, 无采样数据)。
///
/// 用于「有意不合成」的段 (regenIndices 排除 / 空译文 / 无参考音),
/// 取代原先 `fs::write(vec![0u8;44])` 写的*非法零头* wav —— 后者无 RIFF 标记,
/// 会让后续 mix_audio 的 ffprobe/trim 步骤读取失败 (exit 183 "Invalid data")。
///
/// 合法静音 wav 可被 ffprobe 正常探测到 48000Hz, 且时长 0ms, mix_audio 会自然跳过该段。
pub fn write_silent_wav(out_path: &str) -> anyhow::Result<()> {
    voxcpm_cloud::write_wav(&[], SILENT_SAMPLE_RATE, out_path)
        .map_err(|e| anyhow::anyhow!("写静音 wav {} 失败: {e}", out_path))
}

/// 选择 voxcpm-burn 后端二进制名 (镜像 TS runtime/device -> 二进制)。
/// 优先返回 workspace 中已构建的二进制。
///
/// 仅用于非 cloud 运行时 (cloud 在 `step_tts` 中直接走 `VoxCPMCloud`, 不调用本函数)。
/// 因此这里只按 device 选 GPU 后端, runtime 参数仅用于报错信息。
///
/// voxcpm-burn 已迁至 vox-lab. 未来由预编译 release 下载提供 (`voxcpm-burn-{backend}`),
/// 此处仅检查本机是否已有二进制 (含未来 release 下载落地的 `data/bin`), 不再自动本地编译。
pub fn pick_voxcpm_bin(device: TtsDevice, _runtime: TtsRuntime) -> anyhow::Result<String> {
    let candidates: Vec<&str> = match device {
        TtsDevice::Cpu => vec!["voxcpm-burn-cpu"],
        TtsDevice::Rocm => vec!["voxcpm-burn-vulkan", "voxcpm-burn-wgpu"],
        TtsDevice::Mps => vec!["voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        TtsDevice::Webgpu => vec!["voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
        TtsDevice::Cuda => vec!["voxcpm-burn-vulkan", "voxcpm-burn-wgpu", "voxcpm-burn-cpu"],
    };
    for c in &candidates {
        if let Some(p) = find_release_bin(c) {
            return Ok(p.to_string_lossy().into_owned());
        }
    }
    // voxcpm-burn 正在迁移至 vox-lab (预编译 release), 暂不支持本地运行。
    tracing::warn!(target: "tts",
        "未找到 voxcpm-burn 二进制, voxcpm-burn 正在迁移中: 本地运行时暂不支持, 请使用 cloud 运行时 (tts.runtime = \"cloud\")"
    );
    Err(anyhow::anyhow!(
        "voxcpm-burn 正在迁移中: 未找到本地二进制 (候选: {}), 请使用 cloud 运行时",
        candidates.join(", ")
    ))
}

/// 段级失败不允许静默成功 (镜像 translate 的 partial+Err 模式)。
///
/// 静音占位段**已写盘** (所以 `regen_tts` 可针对失败段局部重合成), 但整个 step
/// 视为失败——否则会产出"缺台词却标 success"的产物, 把错误推迟到 `mix_audio`
/// 才爆 (历史上 `mix_audio` 报"没有可合并的音频段", 真凶其实是 tts 全段空)。
///
/// `skipped` / `empty` **不算失败**:
/// - `skipped` = 复用已有有效音频 (见 [`should_skip_segment`])
/// - `empty`   = 原文为空, 合法静音
pub fn ensure_no_failed_segments(result: &TtsFile) -> anyhow::Result<()> {
    let failed = result
        .segments
        .iter()
        .filter(|seg| seg.status == "error")
        .count();
    if failed > 0 {
        anyhow::bail!("TTS 有 {failed} 段合成失败 (静音占位), 已写 tts.json, 请对该段重新生成");
    }
    Ok(())
}

/// 该段能否复用已有 wav (跳过合成)?
///
/// 五个条件全满足才跳过——**每一条都是必要的**:
///
/// | 条件 | 少了会怎样 |
/// | --- | --- |
/// | `skip_existing` | 用户想强制重来时不该被复用 |
/// | `!regen_active` | `regenIndices` 命中的段必须重跑 |
/// | `out_exists` | 没产物当然要合成 |
/// | `out_mtime > ref_mtime` | 参考音比结果新 → 结果已过期 |
/// | `out_dur > 0` | **零时长/损坏 wav 不能复用**, 否则带坏文件进 `mix_audio` (exit 183) |
///
/// 最后一条最关键: 历史上 tts 全部失败时留下 44 字节的空 wav, 若只看
/// "文件存在 + mtime 较新" 就会被复用, 错误一路藏到 `mix_audio`。
pub fn should_skip_segment(
    skip_existing: bool,
    regen_active: bool,
    out_exists: bool,
    out_mtime_ms: u64,
    ref_mtime_ms: u64,
    out_duration_ms: u64,
) -> bool {
    skip_existing
        && !regen_active
        && out_exists
        && out_mtime_ms > ref_mtime_ms
        && out_duration_ms > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::tts::out::TtsSegment;

    /// 构造一个只有 `status` 不同的段 (其余字段测试不关心)。
    fn seg(status: &str) -> TtsSegment {
        TtsSegment {
            timing: crate::steps::split_audio::out::SplitAudioTiming {
                idx: 1,
                text: "t".into(),
                start_ms: 0,
                end_ms: 100,
                dst: "d".into(),
                src_lang: None,
                dst_lang: None,
                speaker: None,
            },
            slot_end_ms: 100,
            tts_duration_ms: 100,
            status: status.to_string(),
        }
    }

    fn file(statuses: &[&str]) -> TtsFile {
        TtsFile {
            segments: statuses.iter().map(|s| seg(s)).collect(),
        }
    }

    /// **事故根因护栏**: 有失败段就必须硬失败, 不许标 success。
    ///
    /// 历史上 tts 全段失败却标 success, 错误推迟到 `mix_audio` 才以
    /// "没有可合并的音频段" 爆出来——真凶是 tts, 受害者是 mix_audio。
    #[test]
    fn any_failed_segment_makes_step_fail() {
        assert!(ensure_no_failed_segments(&file(&["success"])).is_ok());
        // skipped / empty 都是合法终态, 不算失败
        assert!(ensure_no_failed_segments(&file(&["skipped"])).is_ok());
        assert!(ensure_no_failed_segments(&file(&["empty"])).is_ok());
        assert!(ensure_no_failed_segments(&file(&["success", "skipped", "empty"])).is_ok());
        assert!(ensure_no_failed_segments(&file(&[])).is_ok());

        // 只要有一段 error → 失败, 且信息里带段数
        let err = ensure_no_failed_segments(&file(&["success", "error"]))
            .expect_err("有 error 段必须失败");
        assert!(
            err.to_string().contains("1 段合成失败"),
            "应报出失败段数, 实际: {err}"
        );

        // 35 段全失败 (真实事故场景)
        let all_err: Vec<&str> = vec!["error"; 35];
        let err = ensure_no_failed_segments(&file(&all_err)).expect_err("全失败必须失败");
        assert!(err.to_string().contains("35 段合成失败"), "实际: {err}");
    }

    /// 五个条件的 AND: 缺任何一条都不该跳过。
    #[test]
    fn skip_requires_all_five_conditions() {
        // 基准: 全满足 → 跳过
        assert!(should_skip_segment(true, false, true, 200, 100, 500));

        assert!(
            !should_skip_segment(false, false, true, 200, 100, 500),
            "skipExisting=false 不该跳过"
        );
        assert!(
            !should_skip_segment(true, true, true, 200, 100, 500),
            "regenIndices 命中不该跳过"
        );
        assert!(
            !should_skip_segment(true, false, false, 200, 100, 500),
            "产物不存在不该跳过"
        );
        assert!(
            !should_skip_segment(true, false, true, 100, 200, 500),
            "参考音比产物新 → 产物已过期, 不该跳过"
        );
        assert!(
            !should_skip_segment(true, false, true, 100, 100, 500),
            "mtime 相等不算更新"
        );
    }

    /// **关键兜底**: 零时长 / 损坏的 wav 即使存在且更新, 也**不能**复用。
    ///
    /// 这正是 tts 全失败事故里的形态——44 字节空 wav, 文件在、mtime 新,
    /// 但 data chunk 长度为 0。复用它就会带着坏文件进 `mix_audio`。
    #[test]
    fn zero_duration_wav_is_never_reused() {
        assert!(
            !should_skip_segment(true, false, true, 200, 100, 0),
            "零时长 wav 必须重新合成, 绝不能复用"
        );
        // 1ms 也算有效 (边界)
        assert!(should_skip_segment(true, false, true, 200, 100, 1));
    }
}
