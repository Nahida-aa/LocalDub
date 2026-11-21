//! 引擎 `StepCtx` 的**过渡镜像**（aa-workflow 接入前的本地形状）。
//!
//! 字段/方法与 `aa_workflow_core::StepCtx<I, S>` 的真实形状**同名同位**：
//! `sc.ctx.input`（typed run input）+ `sc.ctx.emit(name, value)` + `sc.id` +
//! `sc.attempt`——接入日纯类型替换（`MyStepCtx → StepCtx<Input, TState>`），
//! 步骤 body 零改动。
//!
//! 刻意**不包含**的东西（依据 docs/workflow/INTEGRATION-PLAN.md §5.4 勘察结论）：
//!
//! - `state` 位：LocalDub 步骤零 state 读取（15 步 0 次），且 `TState` 形状
//!   未拍板——接入日由引擎 `RunCtx.state` 补位，步骤 body 不受影响；
//! - **便捷方法**（`video_dir()` 之类）：`video_dir` 从 `ctx.input.workflow.video_dir`
//!   取（import 身份回填的事实，见 download.rs 的回填注释）——不提前造便捷，
//!   镜像形状以「接入日零 body 改动」为唯一准则。
//!
//! `emit` 的落点：Phase 1 为 no-op；Phase 2 接本地事件通道（投影器折叠成
//! view 文件）；接入日 = 引擎 publisher。签名始终同 `RunCtx::emit`。

/// 过渡期步骤上下文（引擎 `StepCtx<Input, TState>` 的镜像）。
#[derive(Clone)]
pub struct MyStepCtx {
    /// run 级只读视图（≡ 引擎 `StepCtx::ctx: RunCtx`）。
    pub ctx: MyRunCtx,
    /// 幂等键候选（引擎同名同义：跨 retry / replay 稳定）。
    pub id: String,
    /// 当前 attempt（1-based）。过渡执行器无重试，恒为 1。
    pub attempt: usize,
}

/// run 级只读视图（镜像 `aa_workflow_core::RunCtx` 的公开面）。
#[derive(Clone)]
pub struct MyRunCtx {
    /// import 回填后的 run 配置（≡ 引擎 `RunCtx::input`；`workflow.video_dir`
    /// 由 import 身份回填，见 `workflows/import/download.rs`）。
    pub input: crate::input::Input,
}

impl MyRunCtx {
    /// 观测出口（**签名同 `RunCtx::emit`**：发送、不追加任何日志/文件）。
    ///
    /// Phase 1 为 no-op；Phase 2 接本地事件通道——投影器折叠成 view 文件；
    /// 接入日 = 引擎 publisher（同一投影器，换事件源）。
    pub fn emit(&self, name: impl AsRef<str>, value: serde_json::Value) {
        let _ = (name.as_ref(), &value);
        tracing::debug!(target: "step_ctx", "emit (no-op in Phase 1)");
    }
}

impl MyStepCtx {
    /// 执行器在步骤起止构造（引擎同位：`exec_step` 每 attempt 现建）。
    ///
    /// 过渡执行器无重试，`attempt` 恒为 1；接入日 attempt 序号由引擎携带
    /// （跨驱动接续，见 aa-workflow 的 durable retry）。
    pub fn new(id: impl Into<String>, input: crate::input::Input) -> Self {
        Self {
            ctx: MyRunCtx { input },
            id: id.into(),
            attempt: 1,
        }
    }
}
