//! sf_ocr 阶段 (镜像 TS `packages/core/stages/sf_ocr/`)。
//!
//! 关键帧策略: sf_ocr_pre (找关键帧) → sf_ocr (逐帧 OCR) → sf_ocr_fix (合并/修正)。
//! 三者均为 spawn 已构建的 Rust 二进制 (sf-cli / subtitle-ocr / ocr-post) 的编排。

pub mod args;
pub mod fix_args;
pub mod ocr;
pub mod ocr_fix;
pub mod ocr_pre;

pub use ocr::stage_sf_ocr;
pub use ocr_fix::stage_sf_ocr_fix;
pub use ocr_pre::stage_sf_ocr_pre;
