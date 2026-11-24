//! 环境检测项的身份与元信息 (镜像 TS `packages/core/cmd/env/input.ts` 的 `envDescribeMap`)。
//!
//! `EnvKey` 是环境项的**唯一身份**：key 字符串、元数据、check 行为、可选 ensure
//! 行为都挂在它上面（行为在 `items.rs` 的 `impl EnvKey`）。编译期封闭集合，
//! 不再维护三份手工对齐的 key 名单。
//!
//! wire 名由 `strum(serialize_all = "snake_case")` 派生；少数 heck 边界不直观的
//! 变体显式 `serialize` 钉住，并有 roundtrip 单测保护。

use strum::{EnumIter, EnumString, IntoStaticStr};

/// 单个环境项的元信息。
#[derive(Debug, Clone, Copy)]
pub struct EnvEntry {
    pub zh: &'static str,
    pub en: &'static str,
    pub required: bool,
    pub category: &'static str,
}

/// 环境项身份。变体顺序即 env list 输出顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EnvKey {
    Bun,
    Python,
    Uv,
    Ffmpeg,
    Cargo,
    Vulkan,
    Rocm,
    Cuda,
    WhisperGgml,
    WhisperVad,
    DemucsPth,
    #[strum(serialize = "voxcpm2_onnx")]
    Voxcpm2Onnx,
    #[strum(serialize = "voxcpm2_pth")]
    Voxcpm2Pth,
    WhisperBin,
    DemucsBurnBin,
    DemucsBurnTchBin,
    DemucsBurnWgpuBin,
    DemucsWgpuBin,
    DemucsTchCpuBin,
    DemucsTchCudaBin,
    SubtitleFinderBin,
    SubtitleOcrBin,
    SubtitleOcrPostBin,
    Cmake,
    #[strum(serialize = "yt_dlp")]
    YtDlp,
    Git,
    Dotenv,
    Openai,
}

impl EnvKey {
    /// 全部环境项，顺序即声明顺序（取代 `env_names()`）。
    pub fn all() -> Vec<Self> {
        use strum::IntoEnumIterator;
        <Self as IntoEnumIterator>::iter().collect()
    }

    /// wire 名（取代手写 key 字符串）。
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }

    /// 从 wire 名解析；非法 key 在边界处暴露，不再静默跳过。
    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// 元信息（取代 `ENV_ENTRIES` 表与 `zh_desc()`）。穷尽 match：新增变体漏写即编译失败。
    pub fn meta(&self) -> EnvEntry {
        match self {
            Self::Bun => EnvEntry {
                zh: "ts(nodejs) 运行时, 包管理器",
                en: "ts(nodejs) runtime, package manager",
                required: true,
                category: "core",
            },
            Self::Python => EnvEntry {
                zh: "python 运行时 (>= 3.10)",
                en: "python runtime (>= 3.10)",
                required: true,
                category: "core",
            },
            Self::Uv => EnvEntry {
                zh: "python 包管理器",
                en: "python package manager",
                required: true,
                category: "core",
            },
            Self::Ffmpeg => EnvEntry {
                zh: "视频/音频处理工具 (需 libx264, libmp3lame)",
                en: "video/audio processing tool (with libx264, libmp3lame)",
                required: true,
                category: "core",
            },
            Self::Cargo => EnvEntry {
                zh: "rust 包管理器, 编译 burn 后端需要",
                en: "rust package manager, needed for building burn backends",
                required: false,
                category: "optional",
            },
            Self::Vulkan => EnvEntry {
                zh: "vulkan GPU 驱动",
                en: "vulkan GPU driver",
                required: false,
                category: "optional",
            },
            Self::Rocm => EnvEntry {
                zh: "rocm GPU 驱动 (AMD)",
                en: "rocm GPU driver (AMD)",
                required: false,
                category: "optional",
            },
            Self::Cuda => EnvEntry {
                zh: "nvidia CUDA 驱动 + nvidia-smi",
                en: "nvidia CUDA driver + nvidia-smi",
                required: false,
                category: "optional",
            },
            Self::WhisperGgml => EnvEntry {
                zh: "whisper.cpp ggml 模型 (data/models/whisper/ggml-large-v3-turbo.bin)",
                en: "whisper.cpp ggml model (data/models/whisper/ggml-large-v3-turbo.bin)",
                required: false,
                category: "optional",
            },
            Self::WhisperVad => EnvEntry {
                zh: "silero VAD 模型 (data/models/whisper/ggml-silero-v6.2.0.bin)",
                en: "silero VAD model (data/models/whisper/ggml-silero-v6.2.0.bin)",
                required: false,
                category: "optional",
            },
            Self::DemucsPth => EnvEntry {
                zh: "demucs safetensors 模型, 用于 separate burn 后端",
                en: "demucs safetensors model, used for separate burn backend",
                required: false,
                category: "optional",
            },
            Self::Voxcpm2Onnx => EnvEntry {
                zh: "voxcpm2 onnx 模型文件 (4 对), 用于 onnx 后端",
                en: "voxcpm2 onnx model files (4 pairs), used for onnx backend",
                required: false,
                category: "optional",
            },
            Self::Voxcpm2Pth => EnvEntry {
                zh: "voxcpm2 模型 (model.safetensors + audiovae.pth), 用于 tts",
                en: "voxcpm2 model (model.safetensors + audiovae.pth), used for tts",
                required: false,
                category: "optional",
            },
            Self::WhisperBin => EnvEntry {
                zh: "whisper-vulkan 预编译二进制 (Nahida-aa/vox-lab release)",
                en: "whisper-vulkan precompiled binary (Nahida-aa/vox-lab release)",
                required: false,
                category: "optional",
            },
            Self::DemucsBurnBin => EnvEntry {
                zh: "demucs-burn 非发布后端 (cpu/cuda/vulkan 无 release 资产, 请用 tch/wgpu)",
                en: "demucs-burn unpublished backend (cpu/cuda/vulkan have no release asset, use tch/wgpu)",
                required: false,
                category: "optional",
            },
            Self::DemucsBurnTchBin => EnvEntry {
                zh: "demucs-burn tch 后端 (data/bin/demucs-burn-tch, 从 vox-lab GitHub Release 下载, libtorch 平铺同目录)",
                en: "demucs-burn tch backend (data/bin/demucs-burn-tch, downloaded from vox-lab GitHub Release, libtorch flat in same dir)",
                required: false,
                category: "optional",
            },
            Self::DemucsBurnWgpuBin => EnvEntry {
                zh: "demucs-burn wgpu 后端 (data/bin/demucs-burn-wgpu, 从 vox-lab GitHub Release 下载)",
                en: "demucs-burn wgpu backend (data/bin/demucs-burn-wgpu, downloaded from vox-lab GitHub Release)",
                required: false,
                category: "optional",
            },
            Self::DemucsWgpuBin => EnvEntry {
                zh: "demucs-wgpu 独立运行时 (data/bin/demucs-wgpu, 自研 WGSL/Vulkan, 不依赖 libtorch)",
                en: "demucs-wgpu standalone runtime (data/bin/demucs-wgpu, in-house WGSL/Vulkan, no libtorch)",
                required: false,
                category: "optional",
            },
            Self::DemucsTchCpuBin => EnvEntry {
                zh: "demucs-tch cpu 变体 (data/bin/demucs-tch-cpu, 独立 tch-rs 运行时; 尚未发布)",
                en: "demucs-tch cpu variant (data/bin/demucs-tch-cpu, standalone tch-rs; not released yet)",
                required: false,
                category: "optional",
            },
            Self::DemucsTchCudaBin => EnvEntry {
                zh: "demucs-tch cuda 变体 (data/bin/demucs-tch-cuda, 需自备 CUDA 版 libtorch; 尚未发布)",
                en: "demucs-tch cuda variant (data/bin/demucs-tch-cuda, needs CUDA libtorch; not released yet)",
                required: false,
                category: "optional",
            },
            Self::SubtitleFinderBin => EnvEntry {
                zh: "关键帧筛选二进制 (data/bin/subtitle-finder, 从 vision-lab GitHub Release 下载)",
                en: "subtitle-finder binary (data/bin/subtitle-finder, downloaded from vision-lab GitHub Release)",
                required: false,
                category: "optional",
            },
            Self::SubtitleOcrBin => EnvEntry {
                zh: "字幕 OCR 二进制 (data/bin/subtitle-ocr, 从 vision-lab GitHub Release 下载)",
                en: "subtitle-ocr binary (data/bin/subtitle-ocr, downloaded from vision-lab GitHub Release)",
                required: false,
                category: "optional",
            },
            Self::SubtitleOcrPostBin => EnvEntry {
                zh: "字幕后处理统合管线二进制 (data/bin/subtitle-ocr-post, 从 vision-lab GitHub Release 下载)",
                en: "subtitle-ocr-post binary (data/bin/subtitle-ocr-post, downloaded from vision-lab GitHub Release)",
                required: false,
                category: "optional",
            },
            Self::Cmake => EnvEntry {
                zh: "cmake 构建工具, 编译 C++",
                en: "cmake build tool, needed for compiling C++",
                required: false,
                category: "optional",
            },
            Self::YtDlp => EnvEntry {
                zh: "yt-dlp 视频下载工具 (YouTube/Bilibili 链接下载; YTDLP_BIN 覆盖路径)",
                en: "yt-dlp video downloader (YouTube/Bilibili download; override with YTDLP_BIN)",
                required: false,
                category: "optional",
            },
            Self::Git => EnvEntry {
                zh: "git 版本控制, 子模块操作需要",
                en: "git version control, needed for submodule operations",
                required: false,
                category: "optional",
            },
            Self::Dotenv => EnvEntry {
                zh: ".env 配置文件, 包含 DEVICE, API 密钥等",
                en: ".env configuration file with DEVICE, API keys, etc.",
                required: false,
                category: "recommended",
            },
            Self::Openai => EnvEntry {
                zh: "翻译用的 OpenAI 兼容 API (如 Ollama, vLLM, OpenAI)",
                en: "openai-compatible API for translation (e.g. Ollama, vLLM, OpenAI)",
                required: false,
                category: "optional",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_roundtrip() {
        assert_eq!(EnvKey::all().len(), 28);
        for key in EnvKey::all() {
            assert_eq!(EnvKey::parse(key.as_str()), Some(key), "wire 名往返: {}", key.as_str());
        }
        // heck 边界不直观的几个名字，显式钉住
        assert_eq!(EnvKey::Voxcpm2Onnx.as_str(), "voxcpm2_onnx");
        assert_eq!(EnvKey::Voxcpm2Pth.as_str(), "voxcpm2_pth");
        assert_eq!(EnvKey::YtDlp.as_str(), "yt_dlp");
        assert_eq!(EnvKey::DemucsTchCudaBin.as_str(), "demucs_tch_cuda_bin");
        assert_eq!(EnvKey::Openai.as_str(), "openai");
    }
}
