//! workflow 状态：`state.json` + `input.json` + `<step>/state.json` 的读写与类型。
//!
//! # 布局（单写者原则——并发写竞争在结构上消失，不加锁/队列/原子写）
//!
//! ```text
//! <video_dir>/state.json           身份 + 运行状态：workflow 子树（编排器写）
//!                                  + import 事实（frame_rate、video/audio_source_path）
//! <video_dir>/input.json           本次运行的输入配置（[`Input`]）
//! <video_dir>/<step>/state.json    该步骤**唯一写入方**：status/progress/times/messages
//!                                  + facts 私有事实
//! <video_dir>/workflow-engine/     事件日志（引擎真源，见 FsRunStore）——不变
//! ```
//!
//! 三个状态文件各自只有一个写入方：
//!
//! | 文件                  | 写入方                                       |
//! | --------------------- | -------------------------------------------- |
//! | `state.json`          | import（初次）、编排器（`set_workflow`，run 边界） |
//! | `input.json`          | import（初次）、continue / regen_tts（改写配置）   |
//! | `<step>/state.json`   | 该步骤自己                                   |
//!
//! **步骤从不写 `state.json` / `input.json`**——步骤对世界的写入只落在自己的
//! `<step>/state.json`，并行分支因此互不踩。
//!
//! `input` 单独成文件（而不是塞在 `state.json` 里）是因为两者的写者与生命周期不同：
//! 配置会被 continue/regen 改写，身份与事实不会。混在一个文件里就得让两个写者
//! 抢同一个文件的 RMW。
//!
//! **没有「组合态」**：不再提供把三份文件拼成一个大对象的 `read_ctx(&State)`。
//! 消费者各自读自己需要的那一份（多数步骤只需 `video_dir` + `read_input`）——
//! 「顺手全组合」的代价是读者看不出自己到底依赖了谁。
//!
//! # 与引擎真源的关系
//!
//! **这三个文件是投影，事件日志是真源**——引擎的 resume / continue_from /
//! target_step 全部依据日志，不读它们。投影可丢弃、可重建。
//!
//! > ⚠️ 别把这里的 `ctx.state` 和 aa-workflow 引擎的 `ctx.state` 混为一谈：
//! > 名字像而已，后者是 handler 执行期内存。

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use time::FrameRate;
pub mod lang;
pub mod types;
use crate::input::Input;
use crate::workflows::args::StepName;
use strum::IntoEnumIterator as _;
pub use types::{StepStatus, WorkflowStep};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum VideoSource {
    Youtube,
    Bilibili,
    Local,
    Remote,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct WorkflowBrief {
    pub id: String,
    pub title: Option<String>,
    pub status: String,
    pub current_step: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
}

/// `state.json` 的 workflow 子树：任务身份 + 运行状态（编排器专用写）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Workflow {
    pub id: String,
    pub source: VideoSource,
    pub url: String,
    pub title: Option<String>,
    pub status: String,
    pub current_step: Option<String>,
    pub video_dir: String,
    pub error_message: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

impl From<Workflow> for WorkflowBrief {
    fn from(t: Workflow) -> Self {
        Self {
            id: t.id,
            title: t.title,
            status: t.status,
            current_step: t.current_step,
            created_at: t.created_at,
            started_at: t.started_at,
            completed_at: t.completed_at,
            error_message: t.error_message,
        }
    }
}

/// `<video_dir>/state.json`：身份 + 运行状态 + import 事实。
///
/// 不含 `input`（配置单独放 `input.json`），也不含任何步骤产物
/// （`final_video_path` 属 `<mix_video>/state.json` 的 facts）。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkflowState {
    pub workflow: Workflow,
    pub output_mode: String,
    pub frame_rate: FrameRate,
    pub video_source_path: Option<String>,
    pub audio_source_path: Option<String>,
}

/// `<step>/state.json`：单个步骤的**执行状态**（该步骤唯一写入方）。
///
/// 只装「跑到哪了」——产出去哪儿/是什么不在这里，那是 `<step>/out.json`
/// 的 `<Step>Output`（见 [`step_out_path`]）。两者生命周期不同：状态在 step
/// 期间反复 patch，out 是做完那一刻写一次。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Type)]
pub struct StepState {
    #[serde(default)]
    pub status: StepStatus,
    #[serde(default)]
    pub progress: Option<f64>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub last_message: Option<String>,
    #[serde(default)]
    pub error_message: Option<String>,
}

/// 全部步骤状态的集合（[`read_steps_state`] 的产物）。
///
/// newtype 而非裸 `BTreeMap`：类型名把「这是步骤状态表」写进调用点，
/// 免得和其它 `BTreeMap<StepName, _>` 混淆。
#[derive(Debug, Clone, Default)]
pub struct StepsState(pub BTreeMap<StepName, StepState>);

impl StepsState {
    /// 某步骤的状态；未落盘 = 全默认 pending。
    pub fn step(&self, step: StepName) -> StepState {
        self.0.get(&step).cloned().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// 路径
// ---------------------------------------------------------------------------

pub fn state_path(video_dir: &str) -> PathBuf {
    PathBuf::from(video_dir).join("state.json")
}

pub fn input_path(video_dir: &str) -> PathBuf {
    PathBuf::from(video_dir).join("input.json")
}

pub fn step_state_path(video_dir: &str, step_name: &str) -> PathBuf {
    PathBuf::from(video_dir).join(step_name).join("state.json")
}

/// `<step>/out.json`：该步骤的**产物事实**（`<Step>Output` 的落点）。
///
/// 与 `<step>/state.json` 并列：一个装「跑到哪了」，一个装「产出了什么」——
/// 前者的写者是编排器与该步骤的 patch，后者只有该步骤成功后写一次。
pub fn step_out_path(video_dir: &str, step_name: &str) -> PathBuf {
    PathBuf::from(video_dir).join(step_name).join("out.json")
}

// ---------------------------------------------------------------------------
// state.json
// ---------------------------------------------------------------------------

/// 读 `state.json`（宽松：缺字段回退默认，不因单个字段崩溃）。
pub fn read_state(video_dir: &str) -> Result<WorkflowState, String> {
    let path = state_path(video_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    state_from_value(json)
}

/// 从 JSON Value 解析 [`WorkflowState`]（不读写文件）。
pub fn state_from_value(json: serde_json::Value) -> Result<WorkflowState, String> {
    let workflow: Workflow = json
        .get("workflow")
        .ok_or_else(|| "Missing 'workflow' in state".to_string())
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| format!("Failed to parse workflow: {}", e))
        })?;
    Ok(WorkflowState {
        workflow,
        output_mode: json
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("dub")
            .to_string(),
        frame_rate: json
            .get("frame_rate")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(FrameRate::FPS_30),
        video_source_path: json
            .get("video_source_path")
            .and_then(|v| v.as_str())
            .map(String::from),
        audio_source_path: json
            .get("audio_source_path")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

/// 写 `state.json`（import 初次写入 + 编排器 `set_workflow` 专用——步骤不写此文件）。
pub fn write_state(video_dir: &str, state: &WorkflowState) -> Result<(), String> {
    let path = state_path(video_dir);
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize state for {}: {}", video_dir, e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// input.json
// ---------------------------------------------------------------------------

/// 读 `input.json`（本次运行的输入配置）。
///
/// 任务目录由 import 创建并写出 `input.json`；缺失即为非法任务目录，直接报错。
pub fn read_input(video_dir: &str) -> Result<Input, String> {
    let path = input_path(video_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// 写 `input.json`（import 初次写入 + continue/regen 的配置改写）。
pub fn write_input(video_dir: &str, input: &Input) -> Result<(), String> {
    let path = input_path(video_dir);
    let json = serde_json::to_string_pretty(input)
        .map_err(|e| format!("Failed to serialize input for {}: {}", video_dir, e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// <step>/state.json
// ---------------------------------------------------------------------------

/// 读单个步骤的状态文件；**缺失 = 全默认 pending**（import 不预写步骤文件）。
pub fn read_step_state(video_dir: &str, step_name: &str) -> Result<StepState, String> {
    let path = step_state_path(video_dir, step_name);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StepState::default()),
        Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
    };
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// 写单个步骤的状态文件（该步骤的唯一写入方调用）。
pub fn write_step_state(video_dir: &str, step_name: &str, st: &StepState) -> Result<(), String> {
    let path = step_state_path(video_dir, step_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(st)
        .map_err(|e| format!("Failed to serialize step state for {step_name}: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

/// 读 `<step>/out.json` 并反序列化成该步骤的 `<Step>Output`。
///
/// 文件缺失（步骤还没跑完 / 没产物）→ `Ok(None)`；存在但不符合该 Output 的形状
/// → Err（形状错是 bug，不是"还没跑"）。
pub fn read_step_out<T: serde::de::DeserializeOwned>(
    video_dir: &str,
    step_name: &str,
) -> Result<Option<T>, String> {
    let path = step_out_path(video_dir, step_name);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to read {}: {}", path.display(), e)),
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// 写 `<step>/out.json`（该步骤成功跑完、产生了产物事实后的一次性写入）。
pub fn write_step_out<T: serde::Serialize>(
    video_dir: &str,
    step_name: &str,
    out: &T,
) -> Result<(), String> {
    let path = step_out_path(video_dir, step_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(out)
        .map_err(|e| format!("Failed to serialize out for {step_name}: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

/// 读全部步骤的 `<step>/state.json`（CLI `status` 用）。
///
/// 只装**磁盘上真实存在**的步骤——不按序列补 pending 占位：status 问的是
/// 「跑过什么」，补占位会把「没跑」和「pending」混成同一个样子。
pub fn read_steps_state(video_dir: &str) -> Result<StepsState, String> {
    let mut steps = BTreeMap::new();
    for step in StepName::iter() {
        let path = step_state_path(video_dir, step.as_str());
        if !path.exists() {
            continue;
        }
        steps.insert(step, read_step_state(video_dir, step.as_str())?);
    }
    Ok(StepsState(steps))
}

/// **测试专用**：从一个扁平 JSON 一次写出 `state.json` + `input.json`
/// （外加 `asr_language` / `target_language` 两条事实 → 各自 `<step>/out.json`）。
///
/// 运行时没有「扁平 JSON → 一堆文件」这种入口，这是为了不让 20+ 个测试各写一遍
/// 「手工 `write_state` + 手工 `write_input`」的重复劳动而存在的。
///
/// | JSON 键         | 落点                        |
/// | --------------- | --------------------------- |
/// | `workflow` 等   | `state.json`（[`state_from_value`]） |
/// | `input`         | `input.json`（[`Input`]）   |
/// | `asr_language`  | `<asr>/out.json`            |
/// | `target_language` | `<translate>/out.json`    |
///
/// 返回解析出的 [`Input`]，供调用方构造 `<Step>Input::new(dir, &input)`。
#[cfg(test)]
pub fn write_fixture(video_dir: &str, json: serde_json::Value) -> Input {
    fs::create_dir_all(video_dir).expect("fixture 建 video_dir");
    let st = state_from_value(json.clone()).expect("fixture 缺 workflow");
    write_state(video_dir, &st).expect("fixture 写 state.json");
    let input: Input = json
        .get("input")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    write_input(video_dir, &input).expect("fixture 写 input.json");
    if let Some(lang) = json.get("asr_language").and_then(|v| v.as_str()) {
        crate::steps::asr::out::AsrOutput::write(video_dir, lang).expect("fixture 写 asr/out.json");
    }
    if let Some(lang) = json.get("target_language").and_then(|v| v.as_str()) {
        crate::steps::translate::out::TranslateOutput::write(video_dir, lang)
            .expect("fixture 写 translate/out.json");
    }
    input
}

pub fn read_workflow(video_dir: &str) -> Result<Workflow, String> {
    read_state(video_dir).map(|st| st.workflow)
}
