//! workflow 状态：`init.json` + `<step>/state.json` 的读写与类型。
//!
//! # 布局（单写者原则——并发写竞争在结构上消失，不加锁/队列/原子写）
//!
//! ```text
//! <video_dir>/init.json              import 写（一次性）+ 编排器写 workflow 运行状态子树
//!                                    （身份/配置/import 事实：video_source_path 等）
//! <video_dir>/<step>/state.json      该步骤**唯一写入方**（含编排器为它做的边界投影）：
//!                                    status/progress/times/messages + facts 私有事实
//! <video_dir>/workflow-engine/       事件日志（真源，见 FsRunStore）——不变
//! ```
//!
//! **init.json 的写入方**：import（一次性）、编排器（run 边界的 status 子树）、
//! continue/server 的 input 配置改写。**步骤从不写 init.json**——步骤对世界的
//! 写入只落在自己的 `<step>/state.json`。并行分支因此各自写各自的文件，
//! 互踩/撕裂读在结构上不存在。
//!
//! **内存组合态**：[`State`] = `{ init, steps: Map<StepName, StepState> }`。
//! `read_ctx` 每次从盘上组合（step 起点读盘，永远新鲜——translate 中途写的
//! `target_language` 自然对后续可见）；`write_ctx` 分解写回（fixture/透传便捷
//! 入口，运行时路径请用 `set_step` / `set_workflow` / `write_init` 定向写）。
//!
//! # 与引擎真源的关系
//!
//! **`init.json` + 步骤 state 文件是投影，事件日志是真源**——引擎的 resume /
//! continue_from / target_step 全部依据日志，不读这些文件。投影可丢弃、可重建。
//! Phase 3（state 由日志派生）是本布局的后续方向。
//!
//! > ⚠️ 别把这里的 `ctx.state` 和 aa-workflow 引擎的 `ctx.state` 混为一谈：
//! > 名字像而已，后者是 handler 执行期内存。

use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use strum::IntoEnumIterator;
use time::FrameRate;
pub mod lang;
pub mod types;
pub use types::{StepStatus, WorkflowStep};
use crate::input::Input;
use crate::workflows::args::StepName;

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

/// init.json 的 workflow 子树：任务身份 + 运行状态（编排器专用写）。
///
/// `final_video_path` 不在这里——它是 mix_video 步骤的产物事实，
/// 归 `<mix_video>/state.json` 的 facts（前端视图由 `compose_frontend_view`
/// 组合回来）。
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

/// `<video_dir>/init.json`：import 单写的事实与配置 + 编排器的运行状态子树。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct InitState {
    pub workflow: Workflow,
    pub output_mode: String,
    pub input: Input,
    pub frame_rate: FrameRate,
    pub video_source_path: Option<String>,
    pub audio_source_path: Option<String>,
}

/// `<video_dir>/<step>/state.json`：单个步骤的状态（该步骤唯一写入方）。
///
/// `facts` 装步骤私有事实（不走统一 schema）：asr → `asr_language`、
/// translate → `target_language`、mix_video → `final_video_path`。
/// 不出 RPC 面（前端视图是 [`WorkflowState`]），故不 derive `specta::Type`
/// （`serde_json::Value` 的 Type 支持依赖额外 feature）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    #[serde(default)]
    pub facts: serde_json::Value,
}

/// 内存组合态（对齐 `{ init, step_a, step_b, ... }`）：
/// `read_ctx` 从盘上组合，`write_ctx` 分解写回。
/// 不出 RPC 面（前端视图是 [`WorkflowState`]），故不 derive `specta::Type`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub init: InitState,
    pub steps: BTreeMap<StepName, StepState>,
}

/// **前端视图**（`get_workflow_ctx` RPC 的产出，形状 = 旧 state.json）。
/// 由 [`compose_frontend_view`] 从组合态拼装——字段路径与拆分前一致，
/// sdk bindings / 前端零改动。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkflowState {
    pub workflow: Workflow,
    pub steps: Option<Vec<WorkflowStep>>,
    pub output_mode: String,
    pub input: Input,
    pub frame_rate: FrameRate,
    pub video_source_path: Option<String>,
    pub audio_source_path: Option<String>,
    pub asr_language: Option<crate::r#const::lang::LangCode>,
    pub target_language: Option<String>,
}

/// 旧扁平形状（视图）→ 组合态。fixture / 测试便捷桥。
impl From<WorkflowState> for State {
    fn from(v: WorkflowState) -> Self {
        let mut steps = BTreeMap::new();
        for ws in v.steps.iter().flatten() {
            if let Ok(name) = ws.name.parse::<StepName>() {
                steps.insert(
                    name,
                    StepState {
                        status: ws.status,
                        progress: ws.progress,
                        started_at: ws.started_at.clone(),
                        completed_at: ws.completed_at.clone(),
                        last_message: ws.last_message.clone(),
                        error_message: ws.error_message.clone(),
                        facts: serde_json::Value::Null,
                    },
                );
            }
        }
        if let Some(lang) = &v.asr_language {
            steps.entry(StepName::Asr).or_default().facts =
                serde_json::json!({ "asr_language": lang.to_string() });
        }
        if let Some(lang) = &v.target_language {
            steps.entry(StepName::Translate).or_default().facts =
                serde_json::json!({ "target_language": lang });
        }
        State {
            init: InitState {
                workflow: v.workflow,
                output_mode: v.output_mode,
                input: v.input,
                frame_rate: v.frame_rate,
                video_source_path: v.video_source_path,
                audio_source_path: v.audio_source_path,
            },
            steps,
        }
    }
}

/// 组合出前端视图：steps[] 按 `get_steps` 序列排序（name=label 组装）、
/// `workflow.final_video_path` 从 mix_video facts 组合回来、
/// `target_language`/`asr_language` 从各自步骤 facts 组合回来。
pub fn compose_frontend_view(state: &State) -> Result<WorkflowState, String> {
    let seq = crate::steps::get_steps(state)?;
    let steps_view: Vec<WorkflowStep> = seq
        .iter()
        .map(|step| {
            let st = state.step(*step);
            WorkflowStep {
                name: step.as_str().to_string(),
                label: step.as_str().to_string(),
                status: st.status,
                progress: st.progress,
                started_at: st.started_at,
                completed_at: st.completed_at,
                last_message: st.last_message,
                error_message: st.error_message,
            }
        })
        .collect();
    Ok(WorkflowState {
        workflow: Workflow {
            final_video_path: state.final_video_path(),
            ..state.init.workflow.clone()
        },
        steps: Some(steps_view),
        output_mode: state.init.output_mode.clone(),
        input: state.init.input.clone(),
        frame_rate: state.init.frame_rate,
        video_source_path: state.init.video_source_path.clone(),
        audio_source_path: state.init.audio_source_path.clone(),
        asr_language: state.asr_language(),
        target_language: state.target_language(),
    })
}

impl State {
    /// 某步骤的状态；文件缺失 = 全默认 pending。
    pub fn step(&self, step: StepName) -> StepState {
        self.steps.get(&step).cloned().unwrap_or_default()
    }

    /// 读某步骤 facts 里的一个键（只读——并行分支不读未 join 分支的文件，
    /// 依赖图保证读时写者已完成）。
    pub fn step_fact(&self, step: StepName, key: &str) -> Option<serde_json::Value> {
        self.steps.get(&step)?.facts.get(key).cloned()
    }

    /// asr 步骤实测的源语言（`<asr>/state.json` facts）。
    pub fn asr_language(&self) -> Option<crate::r#const::lang::LangCode> {
        self.step_fact(StepName::Asr, "asr_language")?
            .as_str()
            .map(crate::r#const::lang::LangCode::from)
    }

    /// translate 步骤落定的目标语言（`<translate>/state.json` facts）。
    pub fn target_language(&self) -> Option<String> {
        self.step_fact(StepName::Translate, "target_language")?
            .as_str()
            .map(String::from)
    }

    /// mix_video 步骤登记的最终视频（`<mix_video>/state.json` facts）。
    pub fn final_video_path(&self) -> Option<String> {
        self.step_fact(StepName::MixVideo, "final_video_path")?
            .as_str()
            .map(String::from)
    }
}

pub fn init_path(video_dir: &str) -> PathBuf {
    PathBuf::from(video_dir).join("init.json")
}

pub fn step_state_path(video_dir: &str, step_name: &str) -> PathBuf {
    PathBuf::from(video_dir).join(step_name).join("state.json")
}

/// 读 init.json（宽松：input 解析失败回退默认并告警，不崩溃）。
pub fn read_init(video_dir: &str) -> Result<InitState, String> {
    let path = init_path(video_dir);
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    let json: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
    init_from_value(json)
}

/// 从 JSON Value 解析 InitState（不读写文件）。
pub fn init_from_value(json: serde_json::Value) -> Result<InitState, String> {
    let workflow: Workflow = json
        .get("workflow")
        .ok_or_else(|| "Missing 'workflow' in init".to_string())
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| format!("Failed to parse workflow: {}", e))
        })?;
    Ok(InitState {
        workflow,
        output_mode: json
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("dub")
            .to_string(),
        // 宽松加载：input 由 import 以 `Input` 序列化写入；旧版 / TS 产出的
        // 配置若含非法字段，这里不崩溃，回退默认 Input 并告警。
        input: json
            .get("input")
            .cloned()
            .and_then(|v| {
                serde_json::from_value(v)
                    .map_err(|e| {
                        tracing::warn!(target: "ctx", "ctx.input 解析失败, 回退默认 Input: {e}")
                    })
                    .ok()
            })
            .unwrap_or_default(),
        frame_rate: json
            .get("frame_rate")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(time::FrameRate::FPS_30),
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

/// 写 init.json（import / 编排器 / 配置改写专用——步骤不写此文件）。
pub fn write_init(video_dir: &str, init: &InitState) -> Result<(), String> {
    let path = init_path(video_dir);
    let json = serde_json::to_string_pretty(init)
        .map_err(|e| format!("Failed to serialize init for {}: {}", video_dir, e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

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

/// 写单个步骤的 facts 键（RMW 该步骤自己的文件；asr_language /
/// target_language / final_video_path 的写入口）。
pub fn write_step_fact(
    video_dir: &str,
    step_name: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), String> {
    let mut st = read_step_state(video_dir, step_name)?;
    if !st.facts.is_object() {
        st.facts = serde_json::Value::Object(serde_json::Map::new());
    }
    if let Some(obj) = st.facts.as_object_mut() {
        obj.insert(key.to_string(), value);
    }
    write_step_state(video_dir, step_name, &st)
}

/// 组合读：init.json + 各 `<step>/state.json` → [`State`]。
/// 缺失的步骤文件 = pending 默认（不进 map）。
pub fn read_ctx(video_dir: &str) -> Result<State, String> {
    let init = read_init(video_dir)?;
    let mut steps = BTreeMap::new();
    for step in StepName::iter() {
        let path = step_state_path(video_dir, step.as_str());
        if let Ok(raw) = fs::read_to_string(&path) {
            let st: StepState = serde_json::from_str(&raw)
                .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
            steps.insert(step, st);
        }
    }
    Ok(State { init, steps })
}

/// 全量分解写：init.json ← `state.init`、各 step 文件 ← `state.steps`。
///
/// fixture / 透传便捷入口；运行时路径请用 [`write_init`] / `set_step` /
/// `set_workflow` 定向写（避免写放大）。
pub fn write_ctx(video_dir: &str, ctx: &State) -> Result<(), String> {
    write_init(video_dir, &ctx.init)?;
    for (step, st) in &ctx.steps {
        write_step_state(video_dir, step.as_str(), st)?;
    }
    Ok(())
}

/// 从**旧扁平形状**的 JSON Value 解析 State——fixture / 测试便捷入口
/// （运行时读路径是 `read_ctx` 的多文件组合）。
///
/// 映射：`workflow`/`output_mode`/`input`/`frame_rate`/两个 source path →
/// `init`；`steps[]` 数组 → steps map；`target_language` → translate facts；
/// `asr_language` → asr facts。
pub fn read_ctx_from_value(json: serde_json::Value) -> Result<State, String> {
    let workflow: Workflow = json
        .get("workflow")
        .ok_or_else(|| "Missing 'workflow' in state".to_string())
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| format!("Failed to parse workflow: {}", e))
        })?;

    // 宽松加载：input 由 import 以 `Input` 序列化写入；旧版 / TS 产出的
    // 配置若含非法字段（如 TranslateArgs::deny_unknown_fields 拦下的），
    // 这里不崩溃，回退默认 Input 并告警。写回路径仍用严格类型（见 `write_init`）。
    let input = json
        .get("input")
        .cloned()
        .and_then(|v| {
            serde_json::from_value(v)
                .map_err(|e| {
                    tracing::warn!(target: "ctx", "ctx.input 解析失败, 回退默认 Input: {e}")
                })
                .ok()
        })
        .unwrap_or_default();

    let asr_language: Option<crate::r#const::lang::LangCode> = json
        .get("asr_language")
        .and_then(|v| v.as_str())
        .map(crate::r#const::lang::LangCode::from);
    let target_language: Option<String> = json
        .get("target_language")
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut steps = BTreeMap::new();
    if let Some(arr) = json.get("steps").and_then(|v| v.as_array()) {
        for item in arr {
            let Ok(ws) = serde_json::from_value::<WorkflowStep>(item.clone()) else {
                continue;
            };
            let Ok(name) = ws.name.parse::<StepName>() else {
                continue;
            };
            steps.insert(
                name,
                StepState {
                    status: ws.status,
                    progress: ws.progress,
                    started_at: ws.started_at,
                    completed_at: ws.completed_at,
                    last_message: ws.last_message,
                    error_message: ws.error_message,
                    facts: serde_json::Value::Null,
                },
            );
        }
    }
    if let Some(lang) = asr_language {
        steps.entry(StepName::Asr).or_default().facts =
            serde_json::json!({ "asr_language": lang.to_string() });
    }
    if let Some(lang) = target_language {
        steps.entry(StepName::Translate).or_default().facts =
            serde_json::json!({ "target_language": lang });
    }

    Ok(State {
        init: InitState {
            workflow,
            output_mode: json
                .get("output_mode")
                .and_then(|v| v.as_str())
                .unwrap_or("dub")
                .to_string(),
            input,
            frame_rate: json
                .get("frame_rate")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or(time::FrameRate::FPS_30),
            video_source_path: json
                .get("video_source_path")
                .and_then(|v| v.as_str())
                .map(String::from),
            audio_source_path: json
                .get("audio_source_path")
                .and_then(|v| v.as_str())
                .map(String::from),
        },
        steps,
    })
}

pub fn read_workflow(video_dir: &str) -> Result<Workflow, String> {
    read_init(video_dir).map(|init| init.workflow)
}
