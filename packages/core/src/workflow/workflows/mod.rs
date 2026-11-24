//! aa-workflow 声明式 workflow 定义（代码即 DAG）。
//!
//! 每个 workflow 一张具体小图，用 aa-workflow 的 `DagSpec` 声明节点与依赖；
//! handler 由 `dag_to_handler` 生成，不手写第二份编排。

pub mod dub_asr;
pub mod dub_asr_ocr;
pub mod dub_sf_ocr;
pub mod subtitle_asr;
pub mod subtitle_asr_ocr;
pub mod subtitle_sf_ocr;

pub use dub_asr::dub_asr_spec;
pub use dub_asr_ocr::dub_asr_ocr_spec;
pub use dub_sf_ocr::{dub_sf_ocr, dub_sf_ocr_spec};
pub use subtitle_asr::subtitle_asr_spec;
pub use subtitle_asr_ocr::subtitle_asr_ocr_spec;
pub use subtitle_sf_ocr::subtitle_sf_ocr_spec;
