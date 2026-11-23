//! 引擎 `StepCtx`/`RunCtx` 的**过渡镜像**——消费方端口（六边形：端口归消费方）。
//!
//! # 两个 trait 的分工 = 消歧机制
//!
//! - [`CtxView`]：**run 级视图**（镜像引擎 `RunCtx` 公开面）——`ctx` 限定符
//!   标记访问出处：`sc.ctx().input()` 读作「**工作流上下文的输入**」，不是任何
//!   单个步骤的参数。工作流的 input 不切片、不下发、不重命名——它以工作流级
//!   事实的身份经限定路径访问；
//! - [`StepCtxView`]：step 级（`ctx()` 访问器 / `id` / `attempt`）。
//!
//! # 接入日
//!
//! `impl CtxView for RunCtx<Input, TState>` + `impl StepCtxView for
//! StepCtx<Input, TState>`（引擎字段/方法全 pub，各十行）——步骤签名与 body
//! 零改动。关联类型（非 dyn）：`emit` 保持引擎签名 `impl AsRef<str>` 零偏离，
//! 调用点静态分发。
//!
//! 刻意不包含（勘察结论 §5.4）：`state` 位（步骤零读取，TState 待拍板）、
//! 便捷方法（`video_dir` 走 `ctx().input().video_dir()` 既有路径）。

use crate::input::WorkflowInput;

/// run 级视图（镜像 `aa_workflow_core::RunCtx` 公开面）。
pub trait CtxView {
    /// **工作流的输入**（typed，非擦除）——import 回填后的 run 配置。
    ///
    /// 语义注意：这是**工作流级事实**，经 `ctx` 限定访问；不是任何单个步骤
    /// 的参数（步骤的意图参数在 `args.rs` 的 `*Args`，事实面在 `deps`）。
    fn input(&self) -> &WorkflowInput;

    /// 观测出口（**签名同 `RunCtx::emit`**：发送、不追加任何日志/文件）。
    ///
    /// Phase 1 为 no-op；Phase 2 接本地事件通道——投影器折叠成 view 文件；
    /// 接入日 = 引擎 publisher（同一投影器，换事件源）。
    fn emit(&self, name: impl AsRef<str>, value: serde_json::Value);
}

/// step 级视图。
pub trait StepCtxView {
    /// run 级视图——**`ctx` 限定符**标记访问出处（工作流级 vs 步骤级）。
    type Ctx: CtxView;

    fn ctx(&self) -> &Self::Ctx;

    /// 幂等键候选（引擎同名同义：跨 retry / replay 稳定）。
    fn id(&self) -> &str;

    /// 当前 attempt（1-based）。过渡执行器无重试，恒为 1；接入日由引擎携带
    /// （跨驱动接续，见 aa-workflow durable retry）。
    fn attempt(&self) -> usize;
}

/// 过渡期载体（本地执行器在步骤起止构造；引擎同位：`exec_step` 每 attempt 现建）。
#[derive(Clone)]
pub struct MyStepCtx {
    pub ctx: MyRunCtx,
    pub id: String,
    pub attempt: usize,
}

/// 过渡期 run 级视图实体。
#[derive(Clone)]
pub struct MyRunCtx {
    pub input: WorkflowInput,
}

impl CtxView for MyRunCtx {
    fn input(&self) -> &WorkflowInput {
        &self.input
    }

    fn emit(&self, name: impl AsRef<str>, value: serde_json::Value) {
        let _ = (name.as_ref(), &value);
        tracing::debug!(target: "step_ctx", "emit (no-op in Phase 1)");
    }
}

impl StepCtxView for MyStepCtx {
    type Ctx = MyRunCtx;

    fn ctx(&self) -> &Self::Ctx {
        &self.ctx
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn attempt(&self) -> usize {
        self.attempt
    }
}

impl MyStepCtx {
    /// `id` = 步骤名（过渡执行器无重试，attempt 恒 1）。
    pub fn new(id: impl Into<String>, input: WorkflowInput) -> Self {
        Self {
            ctx: MyRunCtx { input },
            id: id.into(),
            attempt: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 接入日：引擎 Ctx 实现同一个消费方端口（见模块文档）。
// I 钉死为 WorkflowInput（CtxView::input 返回 &WorkflowInput），S 泛型——
// DAG 形态下 S 无写入者（LocalDub 用 ()），但 impl 不依赖 S 的任何方法。
// ---------------------------------------------------------------------------

impl<S> CtxView for aa_workflow_core::define::RunCtx<WorkflowInput, S> {
    fn input(&self) -> &WorkflowInput {
        &self.input
    }

    fn emit(&self, name: impl AsRef<str>, value: serde_json::Value) {
        // 完全限定名：trait 方法与 RunCtx 固有方法 emit 同名同义，转发出去。
        aa_workflow_core::define::RunCtx::emit(self, name, value);
    }
}

impl<S> StepCtxView for aa_workflow_core::define::StepCtx<WorkflowInput, S> {
    type Ctx = aa_workflow_core::define::RunCtx<WorkflowInput, S>;

    fn ctx(&self) -> &Self::Ctx {
        &self.ctx
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn attempt(&self) -> usize {
        self.attempt
    }
}
