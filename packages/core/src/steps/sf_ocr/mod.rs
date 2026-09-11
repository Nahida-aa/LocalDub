//! sf_ocr 阶段 (镜像 TS `packages/core/steps/sf_ocr/`)。
//!
//! 关键帧策略: sf_ocr_pre (找关键帧) → sf_ocr (逐帧 OCR) → sf_ocr_fix (合并/修正)。
//! 三者均为 spawn 已构建的 vision-lab release 二进制 (subtitle-finder / subtitle-ocr /
//! ocr-post), 由 env ensure_bin 统一校验下载 (镜像 TS `packages/core/steps/sf_ocr/`)。

pub mod args;
pub mod fix_args;
pub mod ocr;
pub mod ocr_fix;
pub mod ocr_pre;

pub use ocr::step_sf_ocr;
pub use ocr_fix::step_sf_ocr_fix;
pub use ocr_pre::step_sf_ocr_pre;
