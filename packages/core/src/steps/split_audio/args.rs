use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::translate;

/// split_audio 阶段参数 (镜像 TS `packages/core/steps/06_split_audio/args.ts` SplitAudioArgsSchema)
///
/// 默认值带业务含义 (startPadMs=100 / endPadMs=300)。TS 端 zod `.prefault({})`
/// 在写入 ctx.json 前已落定默认值, 因此这里只需处理「对象存在但字段缺」:
/// 用 `#[serde(default = "…")]` 兜底。
///
/// 注意: `#[serde(default = "fn")]` 仅在字段级反序列化时生效; 当父结构用
/// `#[serde(default)]` 整体缺省时会调用 Rust `Default`, 故手写 `impl Default`
/// 以保证两种路径下默认值一致 (vadAlign=false / startPadMs=100 / endPadMs=300)。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SplitAudioArgs {
    /// 是否启用静音检测对齐: 修正 segments 前后静音导致的偏移
    #[serde(default)]
    pub vad_align: bool,
    /// 段落切块前缘 padding (ms), 避免语音被截断
    #[serde(default = "default_start_pad_ms")]
    pub start_pad_ms: u32,
    /// 段落切块后缘 padding (ms), 避免语音被截断
    #[serde(default = "default_end_pad_ms")]
    pub end_pad_ms: u32,
    /// 人声文件路径, 调试使用（作为 [`crate::steps::split_audio::SplitAudioSource::Custom]）
    pub vocals_file_path: Option<String>,
}

impl Default for SplitAudioArgs {
    fn default() -> Self {
        Self {
            vad_align: false,
            start_pad_ms: default_start_pad_ms(),
            end_pad_ms: default_end_pad_ms(),
            vocals_file_path: None,
        }
    }
}

fn default_start_pad_ms() -> u32 {
    100
}

fn default_end_pad_ms() -> u32 {
    300
}

/// split_audio 的**输入契约**：切块要用的音频源 + 字幕时序来源 + 切块参数。
///
/// 消费者只需读这里，不必再自己去 ctx 里拼路径 / 判断文件存在性。
#[derive(Debug, Clone)]
pub struct SplitAudioInput {
    /// 切块时要切哪个音频。
    pub audio: SplitAudioSource,
    /// 字幕时序来源（意图时序 + 文本 + 译文）。
    pub translate: translate::LangPath,
}

/// 切块用的音频源（三选一）。
///
/// **顺序即优先级**（对应 `SplitAudioArgs` 的既有逻辑）：
/// 1. [`Custom`](Self::Custom) —— `splitAudio.vocalsFilePath` 显式指定
/// 2. [`Vocals`](Self::Vocals) —— `separate/target_3_vocals.wav` 人声 stem
/// 3. [`SourceAudio`](Self::SourceAudio) —— `audio_source.wav` 原始音频
///
/// **为什么 `Vocals` 是「可能不存在」的**：subtitle 模式不做 separate，没有
/// 人声 stem，此时回退到原始音频是**正常路径**而非错误。所以存在性检查留在
/// 选择逻辑里（`SplitAudioInput::from_ctx`），不进类型。
///
/// **历史说明（已核实：不构成产物差异）**：`step_split_audio` 旧逻辑的 fallback 是
/// `video_source_path(ctx)`（`video_source.mp4`）。但 `audio_source.wav` 本就是
/// import 阶段 `extract_audio(video_source.mp4)` 产出的同名音轨
/// （pcm_s16le / 44100 / 2ch，见 `workflows/import/download.rs`），两者是同一段音频。
/// 且旧代码里 `cut_audio_range`（`-c copy`）只在 `has_vocals` 时调用，彼时
/// `source_audio` 已被改写为 `separate/target_3_vocals.wav`——
/// `video_source.mp4` 实际只喂给 `probe_duration_ms`，其返回时长与 wav 一致。
/// 故本契约的 `SourceAudio` 与旧 fallback **产物相同**，无需实测比对。
///
/// **`SplitAudioArgs` 已移除 `source_file_path`**：旧逻辑里它被 vocals 检查遮蔽，
/// 无法真正作为「从任意源音频切块」的覆盖（仅在「无 vocals + vad_align」窄路径影响
/// VAD 参考音频），且与 `vocals_file_path` 概念混淆。调试自定义切块源统一走
/// `vocals_file_path` → [`crate::steps::split_audio::SplitAudioSource::Custom`]。
#[derive(Debug, Clone)]
pub enum SplitAudioSource {
    /// 人声 stem `separate/target_3_vocals.wav`。
    ///
    /// 注意是 **`separate/`**（demucs 原始人声），不是 `separate_after/` 的
    /// 混音版——`separate_after` 那个是给 asr 用的。
    Vocals(crate::steps::utils::paths::separate::Target3VocalsPath),
    /// 原始音频 `audio_source.wav`。
    SourceAudio(crate::steps::utils::paths::AudioSourcePath),
    /// 调试用：`splitAudio.vocalsFilePath` 指定的任意路径。
    Custom(PathBuf),
}

impl SplitAudioInput {
    /// 从 ctx 解析：先定音频源（按 `SplitAudioSource` 的优先级），再取翻译产物路径。
    ///
    /// **优先级与存在性检查的现状对齐**（`step_split_audio` 原有逻辑）：
    ///
    /// 1. `splitAudio.vocalsFilePath` 显式指定 → [`Custom`](SplitAudioSource::Custom)
    /// 2. `separate/target_3_vocals.wav` **存在** → [`Vocals`](SplitAudioSource::Vocals)
    /// 3. 否则 → [`SourceAudio`](SplitAudioSource::SourceAudio)（`audio_source.wav`）
    ///
    /// 第 2 步的「存在才用」是刻意的：subtitle 模式不做 separate，没有人声 stem，
    /// 回退到原始音频是**正常路径**。故存在性判断留在这里，类型只表达结论。
    ///
    /// `audio_source_path` 缺失 → Err（与 `video_source_path` 同款）。
    pub fn from_ctx(ctx: &crate::context::WorkflowCtx) -> anyhow::Result<Self> {
        let args = super::read_args(ctx);
        let video_dir = &ctx.workflow.video_dir;

        let audio = match args.vocals_file_path {
            Some(p) => SplitAudioSource::Custom(PathBuf::from(p)),
            None => {
                let vocals = crate::steps::utils::vocals_path(video_dir);
                if vocals.exists() {
                    SplitAudioSource::Vocals(vocals)
                } else {
                    SplitAudioSource::SourceAudio(
                        crate::steps::utils::paths::AudioSourcePath::from_ctx(ctx)?,
                    )
                }
            }
        };

        Ok(Self {
            audio,
            translate: translate::LangPath::from_ctx(ctx)?,
        })
    }
}

impl SplitAudioSource {
    /// 实际要切的那个文件。
    pub fn as_path(&self) -> &std::path::Path {
        match self {
            SplitAudioSource::Vocals(p) => p.as_path(),
            SplitAudioSource::SourceAudio(p) => p.as_path(),
            SplitAudioSource::Custom(p) => p.as_path(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::read_ctx_from_value;

    fn ctx_at(video_dir: &str, input: serde_json::Value) -> crate::context::WorkflowCtx {
        let mut c = read_ctx_from_value(input).unwrap();
        c.workflow.video_dir = video_dir.to_string();
        c
    }

    fn mk(video_dir: &str, split_audio_cfg: serde_json::Value, audio_src: Option<&str>)
        -> crate::context::WorkflowCtx {
        let mut c = ctx_at(
            video_dir,
            serde_json::json!({
                "workflow": {"id":"t","video_dir":video_dir,"url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                "input": {"steps": {"split_audio": split_audio_cfg}}
            }),
        );
        c.target_language = Some("vi".to_string());
        c.audio_source_path = audio_src.map(String::from);
        c
    }

    /// 优先级 1：`vocalsFilePath` 显式指定 → 用它，**无视其它**。
    ///
    /// 即使 `separate/target_3_vocals.wav` 存在（真实文件），也该走 Custom。
    #[test]
    fn explicit_vocals_file_path_wins() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_custom_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::create_dir_all(&dir);

        let c = mk(
            &dir,
            serde_json::json!({"vocalsFilePath": "/tmp/my_vocals.wav"}),
            Some("/x/audio_source.wav"),
        );
        let input = SplitAudioInput::from_ctx(&c).expect("应可构造");
        assert!(matches!(input.audio, SplitAudioSource::Custom(_)));
        assert_eq!(input.audio.as_path().to_string_lossy(), "/tmp/my_vocals.wav");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 优先级 2：`separate/target_3_vocals.wav` **存在** → 用人声 stem。
    #[test]
    fn existing_vocals_stem_is_preferred() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_vocals_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        // 造出 separate/target_3_vocals.wav
        std::fs::create_dir_all(std::path::Path::new(&dir).join("separate")).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("separate").join("target_3_vocals.wav"),
            b"x",
        )
        .unwrap();

        let c = mk(&dir, serde_json::json!({}), Some("/x/audio_source.wav"));
        let input = SplitAudioInput::from_ctx(&c).expect("应可构造");
        assert!(
            matches!(input.audio, SplitAudioSource::Vocals(_)),
            "存在 vocals 时应优先用它, 实际 {:?}",
            input.audio.as_path()
        );
        // 必须是 separate/（原始人声），不是 separate_after/
        assert!(
            input.audio.as_path().to_string_lossy().contains("/separate/"),
            "该用 separate/ 的原始人声: {:?}",
            input.audio.as_path()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 优先级 3：没有 vocals stem → 回退 `audio_source.wav`。
    ///
    /// 这条是 subtitle 模式的**正常路径**（不做 separate），不是错误。
    #[test]
    fn falls_back_to_audio_source_when_no_vocals() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_fallback_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let c = mk(&dir, serde_json::json!({}), Some("/x/audio_source.wav"));
        let input = SplitAudioInput::from_ctx(&c).expect("应可构造");
        assert!(
            matches!(input.audio, SplitAudioSource::SourceAudio(_)),
            "无 vocals 应回退音频源, 实际 {:?}",
            input.audio.as_path()
        );
        assert_eq!(input.audio.as_path().to_string_lossy(), "/x/audio_source.wav");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `audio_source_path` 缺失 + 无 vocals → Err（不猜路径）。
    ///
    /// 与 `video_source_path` 同款语义：没 import 过就没有这个文件。
    #[test]
    fn missing_audio_source_is_error() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_missing_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let c = mk(&dir, serde_json::json!({}), None);
        let err = SplitAudioInput::from_ctx(&c).expect_err("缺 audio_source_path 应报错");
        assert!(
            err.to_string().contains("audio_source_path"),
            "错误应点名字段: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `target_language` 缺失 → Err（翻译产物路径无从确定）。
    #[test]
    fn missing_target_language_is_error() {
        let dir = std::env::temp_dir()
            .join(format!("ld_sa_nolang_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut c = mk(&dir, serde_json::json!({}), Some("/x/audio_source.wav"));
        c.target_language = None;
        let err = SplitAudioInput::from_ctx(&c).expect_err("缺 target_language 应报错");
        assert!(
            err.to_string().contains("target_language"),
            "错误应点名字段: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
