//! separate_after 步骤的参数 + 跨步骤依赖清单(代码即 DAG 视角)。
//!
//! 历史背景: 混音策略字段(mix_mode / reduce_bgm / sidechain_compress / use_gate)
//! 曾挂在 `AsrArgs` 下，由本步骤通过 `input.steps.asr.clone()` 借用——但 asr 步骤
//! 自身从不读这些字段，它们其实是本步骤的职责。现已归位到 `SeparateAfterArgs`
//! 与 `input.steps.separate_after`。

use serde::{Deserialize, Serialize};

use crate::steps::utils::paths::separate;

/// ASR 音频源: vocals=纯分离人声, raw-sum=直接叠加, sidechain=侧链压缩背景音
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum MixMode {
    Vocals,
    #[serde(rename = "raw-sum")]
    RawSum,
    #[default]
    Sidechain,
}

/// 侧链压缩器参数 (镜像 TS `sidechainCompress`)
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct SidechainCompress {
    /// 压缩器阈值, 默认 0.1
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// 压缩比, 默认 20
    #[serde(default = "default_ratio")]
    pub ratio: f64,
    /// attack 时间(ms), 默认 1
    #[serde(default = "default_attack")]
    pub attack: f64,
    /// release 时间(ms), 默认 200
    #[serde(default = "default_release")]
    pub release: f64,
}

impl Default for SidechainCompress {
    fn default() -> Self {
        Self {
            threshold: default_threshold(),
            ratio: default_ratio(),
            attack: default_attack(),
            release: default_release(),
        }
    }
}

fn default_threshold() -> f64 {
    0.1
}

fn default_ratio() -> f64 {
    20.0
}

fn default_attack() -> f64 {
    1.0
}

fn default_release() -> f64 {
    200.0
}

/// separate_after 阶段参数（`input.steps.separate_after`）。
///
/// 混音策略的单一真源。`use_separated` 例外：它同属 asr 的音源决策，仍从
/// `input.steps.asr` 读取（见 `step_separate_after`），不在这里重复配置。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub struct SeparateAfterArgs {
    /// ASR 音频源 (默认 sidechain)
    #[serde(default = "default_mix_mode")]
    pub mix_mode: MixMode,
    /// 背景音降低量(dB); raw-sum 叠加前衰减, sidechain 时压缩后额外衰减; 默认 -12
    #[serde(default = "default_reduce_bgm")]
    pub reduce_bgm: f64,
    /// mix_mode=sidechain 时侧链压缩器参数
    #[serde(default = "default_sidechain_compress")]
    pub sidechain_compress: SidechainCompress,
    /// 对分离后的人声应用 silence gate 过滤静音段噪声
    #[serde(default)]
    pub use_gate: bool,
}

impl Default for SeparateAfterArgs {
    fn default() -> Self {
        Self {
            mix_mode: default_mix_mode(),
            reduce_bgm: default_reduce_bgm(),
            sidechain_compress: default_sidechain_compress(),
            use_gate: false,
        }
    }
}

fn default_mix_mode() -> MixMode {
    MixMode::Sidechain
}

fn default_reduce_bgm() -> f64 {
    -12.0
}

fn default_sidechain_compress() -> SidechainCompress {
    SidechainCompress::default()
}

/// Phase 1 事实面：`step_separate_after` 的显式依赖清单（由装配层算好后注入）。
///
/// 装 separate 步骤产出的两条路径:
/// - `vocals_path`: `target_3_vocals.wav`(纯人声), 混音 / gate 的输入;
/// - `stems_dir`:   `separate/` 目录, BGM 重建(drums/bass/other 三路)的来源。
///
/// 意图面（`video_dir` / `args` / `use_separated`）走 `sc.ctx().input()`，不进 deps。
#[derive(Debug, Clone)]
pub struct SeparateAfterDeps {
    /// separate 步骤输出的 vocals stem（人声），混音 / gate 的输入。
    pub vocals_path: separate::Target3VocalsPath,
    /// separate 步骤输出的 stems 目录（drums/bass/other/vocals），BGM 重建的来源。
    pub stems_dir: separate::StemsDir,
}

/// `step_separate_after` 的**全部产物清单**（作为步骤函数返回值，不写盘）。
///
/// 三个产物都是条件性的: bgm（stems 齐全才重建）、mixed（use_separated 且
/// mix_mode≠Vocals）、gated（use_gate 且 mixed 已产出）。末尾以文件是否实际存在
/// 为准填入 Option，精确反映本次真实产出。
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct SeparateAfterOutput {
    /// 重建的 BGM `separate_after/target_bgm.wav`。
    pub bgm_path: Option<std::path::PathBuf>,
    /// sidechain / raw-sum 混音人声 `separate_after/target_3_vocals_mixed.wav`。
    pub mixed_path: Option<std::path::PathBuf>,
    /// silence gate 后的混音人声 `separate_after/target_3_vocals_gated.wav`。
    pub gated_path: Option<std::path::PathBuf>,
}

impl SeparateAfterDeps {
    pub fn from_input(input: &crate::input::WorkflowInput) -> Self {
        let video_dir = input.video_dir();
        Self {
            vocals_path: separate::Target3VocalsPath::new(video_dir),
            stems_dir: separate::StemsDir::new(video_dir),
        }
    }
}
