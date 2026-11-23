//! aa-workflow 声明式 workflow 定义（代码即 DAG）。
//!
//! 每个 workflow 一张具体小图，用 aa-workflow 的 `DagSpec` 声明节点与依赖；
//! handler 由 `dag_to_handler` 生成，不手写第二份编排。与 `workflow/dag.rs`
//! 的 `StepDag` 是同一份事实的两种用途：后者供过渡的 `parallel.rs` 调度，
//! 这里的 spec 供引擎执行。

pub mod dub_sf_ocr;

pub use dub_sf_ocr::{dub_sf_ocr,  dub_sf_ocr_spec};
