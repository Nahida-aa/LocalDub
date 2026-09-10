import { z } from "zod";

export const envDescribeMap = {
  bun: {
    en: "ts(nodejs) runtime, package manager",
    zh: "ts(nodejs) 运行时, 包管理器",
    required: true,
    category: "core",
  },
  python: {
    en: "python runtime (>= 3.10)",
    zh: "python 运行时 (>= 3.10)",
    required: true,
    category: "core",
  },
  uv: {
    en: "python package manager",
    zh: "python 包管理器",
    required: true,
    category: "core",
  },
  ffmpeg: {
    en: "video/audio processing tool (with libx264, libmp3lame)",
    zh: "视频/音频处理工具 (需 libx264, libmp3lame)",
    required: true,
    category: "core",
  },
  cargo: {
    en: "rust package manager, needed for building burn backends",
    zh: "rust 包管理器, 编译 burn 后端需要",
    required: false,
    category: "optional",
  },
  vcpkg: {
    en: "c++ package manager, only needed on windows for OCR build",
    zh: "c++ 包管理器, 仅 windows 上 OCR 编译需要",
    required: false,
    category: "windows-only",
  },
  vulkan: {
    en: "vulkan GPU driver",
    zh: "vulkan GPU 驱动",
    required: false,
    category: "optional",
  },
  rocm: {
    en: "rocm GPU driver (AMD)",
    zh: "rocm GPU 驱动 (AMD)",
    required: false,
    category: "optional",
  },
  cuda: {
    en: "nvidia CUDA driver + nvidia-smi",
    zh: "nvidia CUDA 驱动 + nvidia-smi",
    required: false,
    category: "optional",
  },
  whisper_ggml: {
    en: "whisper.cpp ggml model (data/models/whisper/ggml-large-v3-turbo.bin)",
    zh: "whisper.cpp ggml 模型 (data/models/whisper/ggml-large-v3-turbo.bin)",
    required: false,
    category: "optional",
  },
  whisper_vad: {
    en: "silero VAD model (data/models/whisper/ggml-silero-v6.2.0.bin)",
    zh: "silero VAD 模型 (data/models/whisper/ggml-silero-v6.2.0.bin)",
    required: false,
    category: "optional",
  },
  whisper_sherpa: {
    en: "sherpa-onnx whisper model (data/models/whisper/sherpa_onnx/)",
    zh: "sherpa-onnx whisper 模型 (data/models/whisper/sherpa_onnx/)",
    required: false,
    category: "optional",
  },
  whisper_onnx: {
    en: "onnx-community whisper model (data/models/whisper/encoder_model.onnx)",
    zh: "onnx-community whisper 模型 (data/models/whisper/encoder_model.onnx)",
    required: false,
    category: "optional",
  },
  demucs_pth: {
    en: "demucs safetensors model, used for separate burn backend",
    zh: "demucs safetensors 模型, 用于 separate burn 后端",
    required: false,
    category: "optional",
  },
  demucs_onnx: {
    en: "demucs onnx model files, used for onnx separate",
    zh: "demucs onnx 模型文件, 用于 onnx separate",
    required: false,
    category: "optional",
  },
  demucs_ggml: {
    en: "demucs ggml model (data/models/demucs/ggml-model-htdemucs-4s-f16.bin)",
    zh: "demucs ggml 模型 (data/models/demucs/ggml-model-htdemucs-4s-f16.bin)",
    required: false,
    category: "optional",
  },
  voxcpm2_onnx: {
    en: "voxcpm2 onnx model files (4 pairs), used for onnx backend",
    zh: "voxcpm2 onnx 模型文件 (4 对), 用于 onnx 后端",
    required: false,
    category: "optional",
  },
  voxcpm2_pth: {
    en: "voxcpm2 model (model.safetensors + audiovae.pth), used for tts",
    zh: "voxcpm2 模型 (model.safetensors + audiovae.pth), 用于 tts",
    required: false,
    category: "optional",
  },
  demucs_burn_bin: {
    en: "demucs-burn unpublished backend (cpu/cuda/vulkan have no release asset, use tch/wgpu)",
    zh: "demucs-burn 非发布后端 (cpu/cuda/vulkan 无 release 资产, 请用 tch/wgpu)",
    required: false,
    category: "optional",
  },
  ocr_cpp_bin: {
    en: "OCR C++ compiled binary (packages/subtitle-ocr/ort-cpp/build/)",
    zh: "OCR C++ 编译产物 (packages/subtitle-ocr/ort-cpp/build/)",
    required: false,
    category: "optional",
  },
  cmake: {
    en: "cmake build tool, needed for compiling C++",
    zh: "cmake 构建工具, 编译 C++",
    required: false,
    category: "optional",
  },
  git: {
    en: "git version control, needed for submodule operations",
    zh: "git 版本控制, 子模块操作需要",
    required: false,
    category: "optional",
  },
  dotenv: {
    en: ".env configuration file with DEVICE, API keys, etc.",
    zh: ".env 配置文件, 包含 DEVICE, API 密钥等",
    required: false,
    category: "recommended",
  },
  openai: {
    en: "openai-compatible API for translation (e.g. Ollama, vLLM, OpenAI)",
    zh: "翻译用的 OpenAI 兼容 API (如 Ollama, vLLM, OpenAI)",
    required: false,
    category: "optional",
  },
} as const;

type EnvDescribeMap = typeof envDescribeMap;
export type EnvName = keyof EnvDescribeMap;
export const envList = Object.keys(envDescribeMap) as EnvName[];

export const EnvArgsSchema = z.object({
  action: z.enum(["check", "ensure"]).default("check").optional(),
  targets: z.array(z.enum(envList)).default([]).optional(),
});
