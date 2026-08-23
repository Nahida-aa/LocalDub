//! 环境检测项的元信息表 (镜像 TS `packages/core/cmd/env/input.ts` 的 `envDescribeMap`)。
//!
//! 中文项目, 仅保留 `zh` 描述 (en 一并保留以备扩展)。`ENV_NAME` 即 TS 的 `envList`,
//! 也是 `all_checks` 的全部 key 集合。

/// 单个环境项的元信息。
#[derive(Debug, Clone, Copy)]
pub struct EnvEntry {
    pub zh: &'static str,
    pub en: &'static str,
    pub required: bool,
    pub category: &'static str,
}

/// 全部环境项 (顺序即 env list 输出顺序)。
pub const ENV_ENTRIES: &[(&'static str, EnvEntry)] = &[
    (
        "bun",
        EnvEntry {
            zh: "ts(nodejs) 运行时, 包管理器",
            en: "ts(nodejs) runtime, package manager",
            required: true,
            category: "core",
        },
    ),
    (
        "python",
        EnvEntry {
            zh: "python 运行时 (>= 3.10)",
            en: "python runtime (>= 3.10)",
            required: true,
            category: "core",
        },
    ),
    (
        "uv",
        EnvEntry {
            zh: "python 包管理器",
            en: "python package manager",
            required: true,
            category: "core",
        },
    ),
    (
        "ffmpeg",
        EnvEntry {
            zh: "视频/音频处理工具 (需 libx264, libmp3lame)",
            en: "video/audio processing tool (with libx264, libmp3lame)",
            required: true,
            category: "core",
        },
    ),
    (
        "cargo",
        EnvEntry {
            zh: "rust 包管理器, 编译 burn 后端需要",
            en: "rust package manager, needed for building burn backends",
            required: false,
            category: "optional",
        },
    ),
    (
        "vcpkg",
        EnvEntry {
            zh: "c++ 包管理器, 仅 windows 上 OCR 编译需要",
            en: "c++ package manager, only needed on windows for OCR build",
            required: false,
            category: "windows-only",
        },
    ),
    (
        "vulkan",
        EnvEntry {
            zh: "vulkan GPU 驱动",
            en: "vulkan GPU driver",
            required: false,
            category: "optional",
        },
    ),
    (
        "rocm",
        EnvEntry {
            zh: "rocm GPU 驱动 (AMD)",
            en: "rocm GPU driver (AMD)",
            required: false,
            category: "optional",
        },
    ),
    (
        "cuda",
        EnvEntry {
            zh: "nvidia CUDA 驱动 + nvidia-smi",
            en: "nvidia CUDA driver + nvidia-smi",
            required: false,
            category: "optional",
        },
    ),
    (
        "whisper_ggml",
        EnvEntry {
            zh: "whisper.cpp ggml 模型 (data/models/whisper/ggml-large-v3-turbo.bin)",
            en: "whisper.cpp ggml model (data/models/whisper/ggml-large-v3-turbo.bin)",
            required: false,
            category: "optional",
        },
    ),
    (
        "whisper_vad",
        EnvEntry {
            zh: "silero VAD 模型 (data/models/whisper/ggml-silero-v6.2.0.bin)",
            en: "silero VAD model (data/models/whisper/ggml-silero-v6.2.0.bin)",
            required: false,
            category: "optional",
        },
    ),
    (
        "whisper_sherpa",
        EnvEntry {
            zh: "sherpa-onnx whisper 模型 (data/models/whisper/sherpa_onnx/)",
            en: "sherpa-onnx whisper model (data/models/whisper/sherpa_onnx/)",
            required: false,
            category: "optional",
        },
    ),
    (
        "whisper_onnx",
        EnvEntry {
            zh: "onnx-community whisper 模型 (data/models/whisper/encoder_model.onnx)",
            en: "onnx-community whisper model (data/models/whisper/encoder_model.onnx)",
            required: false,
            category: "optional",
        },
    ),
    (
        "demucs_pth",
        EnvEntry {
            zh: "demucs safetensors 模型, 用于 separate burn 后端",
            en: "demucs safetensors model, used for separate burn backend",
            required: false,
            category: "optional",
        },
    ),
    (
        "demucs_onnx",
        EnvEntry {
            zh: "demucs onnx 模型文件, 用于 onnx separate",
            en: "demucs onnx model files, used for onnx separate",
            required: false,
            category: "optional",
        },
    ),
    (
        "demucs_ggml",
        EnvEntry {
            zh: "demucs ggml 模型 (data/models/demucs/ggml-model-htdemucs-4s-f16.bin)",
            en: "demucs ggml model (data/models/demucs/ggml-model-htdemucs-4s-f16.bin)",
            required: false,
            category: "optional",
        },
    ),
    (
        "voxcpm2_onnx",
        EnvEntry {
            zh: "voxcpm2 onnx 模型文件 (4 对), 用于 onnx 后端",
            en: "voxcpm2 onnx model files (4 pairs), used for onnx backend",
            required: false,
            category: "optional",
        },
    ),
    (
        "voxcpm2_pth",
        EnvEntry {
            zh: "voxcpm2 模型 (model.safetensors + audiovae.pth), 用于 tts",
            en: "voxcpm2 model (model.safetensors + audiovae.pth), used for tts",
            required: false,
            category: "optional",
        },
    ),
    (
        "submodule_whisper_cpp",
        EnvEntry {
            zh: "git 子模块: whisper.cpp",
            en: "git submodule: whisper.cpp",
            required: false,
            category: "optional",
        },
    ),
    (
        "submodule_demucs_cpp",
        EnvEntry {
            zh: "git 子模块: demucs.cpp",
            en: "git submodule: demucs.cpp",
            required: false,
            category: "optional",
        },
    ),
    (
        "submodule_demucs_rs",
        EnvEntry {
            zh: "git 子模块: demucs-rs",
            en: "git submodule: demucs-rs",
            required: false,
            category: "optional",
        },
    ),
    (
        "submodule_voxcpm_rs",
        EnvEntry {
            zh: "git 子模块: voxcpm-rs",
            en: "git submodule: voxcpm-rs",
            required: false,
            category: "optional",
        },
    ),
    (
        "whisper_bin",
        EnvEntry {
            zh: "whisper-vulkan 编译产物 (submodule/whisper.cpp/build/bin/)",
            en: "whisper-vulkan compiled binary (submodule/whisper.cpp/build/bin/)",
            required: false,
            category: "optional",
        },
    ),
    (
        "demucs_ggml_bin",
        EnvEntry {
            zh: "demucs.cpp ggml 编译产物 (submodule/demucs.cpp/build/)",
            en: "demucs.cpp ggml compiled binary (submodule/demucs.cpp/build/)",
            required: false,
            category: "optional",
        },
    ),
    (
        "voxcpm_burn_bin",
        EnvEntry {
            zh: "voxcpm-burn 编译产物 (target/release/voxcpm-burn-*)",
            en: "voxcpm-burn compiled binaries (target/release/voxcpm-burn-*)",
            required: false,
            category: "optional",
        },
    ),
    (
        "demucs_burn_bin",
        EnvEntry {
            zh: "demucs-burn 编译产物 (target/release/demucs-burn-*)",
            en: "demucs-burn compiled binaries (target/release/demucs-burn-*)",
            required: false,
            category: "optional",
        },
    ),
    (
        "ocr_cpp_bin",
        EnvEntry {
            zh: "OCR C++ 编译产物 (packages/subtitle-ocr/ort-cpp/build/)",
            en: "OCR C++ compiled binary (packages/subtitle-ocr/ort-cpp/build/)",
            required: false,
            category: "optional",
        },
    ),
    (
        "cmake",
        EnvEntry {
            zh: "cmake 构建工具, 编译 C++",
            en: "cmake build tool, needed for compiling C++",
            required: false,
            category: "optional",
        },
    ),
    (
        "git",
        EnvEntry {
            zh: "git 版本控制, 子模块操作需要",
            en: "git version control, needed for submodule operations",
            required: false,
            category: "optional",
        },
    ),
    (
        "dotenv",
        EnvEntry {
            zh: ".env 配置文件, 包含 DEVICE, API 密钥等",
            en: ".env configuration file with DEVICE, API keys, etc.",
            required: false,
            category: "recommended",
        },
    ),
    (
        "openai",
        EnvEntry {
            zh: "翻译用的 OpenAI 兼容 API (如 Ollama, vLLM, OpenAI)",
            en: "openai-compatible API for translation (e.g. Ollama, vLLM, OpenAI)",
            required: false,
            category: "optional",
        },
    ),
];

/// 全部环境项 key 集合 (镜像 TS `envList`)。由 `ENV_ENTRIES` 派生, 单一数据源避免漂移。
pub fn env_names() -> Vec<&'static str> {
    ENV_ENTRIES.iter().map(|(k, _)| *k).collect()
}

/// 取某项的中文描述 (找不到返回空串)。
pub fn zh_desc(key: &str) -> &'static str {
    for (k, e) in ENV_ENTRIES {
        if *k == key {
            return e.zh;
        }
    }
    ""
}
