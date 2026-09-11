//! 下拉选项常量: 从 Rust 枚举派生 (specta -> bindings 类型)。
//!
//! `keysOf(... satisfies Record<T, string>)` 全键约束: Rust 枚举加/改值 →
//! gen-ts-sdk → 此处编译报错, 强制同步 (不再手写漏值)。`InputCommand` 无 specta
//! 类型, 保持手写。

import type {
  Pipeline,
  ServerAction,
  ServerType_Serialize,
  StepName,
  SubtitleSource,
  TargetLang,
  WorkflowAction,
} from "@repo/sdk/fnrpc/bindings";

const keysOf = <T extends Record<string, unknown>>(o: T): string[] => Object.keys(o);

const COMMANDS = ["workflow", "servers", "env", "cookie"];

const WORKFLOW_ACTIONS = keysOf({
  start: "",
  continue: "",
  import: "",
  enqueue_start: "",
  enqueue_continue: "",
  enqueue_import: "",
  enqueue_dir: "",
  list_queue: "",
  cancel_queue: "",
  status: "",
  get_group_list: "",
  get_workflow_ctx: "",
  generate_meta: "",
} satisfies Record<WorkflowAction, string>);

const STEPS = [
  "",
  ...keysOf({
    separate: "",
    separate_after: "",
    asr: "",
    asr_fix: "",
    sf_ocr_pre: "",
    sf_ocr: "",
    sf_ocr_fix: "",
    asr_ocr_pre: "",
    asr_ocr: "",
    asr_ocr_fix: "",
    translate: "",
    split_audio: "",
    tts: "",
    mix_audio: "",
    mix_video: "",
  } satisfies Record<StepName, string>),
];

const PIPELINES = ["", ...keysOf({ dub: "", subtitle: "" } satisfies Record<Pipeline, string>)];

const SUBTITLE_SOURCES = [
  "",
  ...keysOf({ asr: "", sf_ocr: "", asr_ocr: "" } satisfies Record<SubtitleSource, string>),
];

const SERVER_ACTIONS = [
  "",
  ...keysOf({ status: "", start: "", stop: "", discovery: "" } satisfies Record<
    ServerAction,
    string
  >),
];

const SERVER_NAMES = [
  "",
  ...keysOf({
    main: "",
    voxcpm_torch_gradio: "",
  } satisfies Record<ServerType_Serialize, string>),
];

// 语言码: 与 Rust `const::lang::LANGS` (bindings TargetLang) 同步。
// sourceLang 值域开放 (whisper ~99 种), 这里只列可翻译的 23 种已知语言,
// 列表外语言码 (it/uk/nl...) 需在 input.jsonc 文本页直接填。
const LANGS = keysOf({
  en: "",
  zh: "",
  vi: "",
  ja: "",
  ko: "",
  fr: "",
  de: "",
  es: "",
  pt: "",
  ru: "",
  ar: "",
  hi: "",
  th: "",
  id: "",
  ms: "",
  tl: "",
  my: "",
  km: "",
  lo: "",
  mn: "",
  ne: "",
  ur: "",
  bn: "",
} satisfies Record<TargetLang, string>);

const LANG_LABELS: Record<string, string> = {
  en: "英语 English",
  zh: "中文 Chinese",
  vi: "越南语 Vietnamese",
  ja: "日语 Japanese",
  ko: "韩语 Korean",
  fr: "法语 French",
  de: "德语 German",
  es: "西班牙语 Spanish",
  pt: "葡萄牙语 Portuguese",
  ru: "俄语 Russian",
  ar: "阿拉伯语 Arabic",
  hi: "印地语 Hindi",
  th: "泰语 Thai",
  id: "印尼语 Indonesian",
  ms: "马来语 Malay",
  tl: "他加禄语 Tagalog",
  my: "缅甸语 Burmese",
  km: "高棉语 Khmer",
  lo: "老挝语 Lao",
  mn: "蒙古语 Mongolian",
  ne: "尼泊尔语 Nepali",
  ur: "乌尔都语 Urdu",
  bn: "孟加拉语 Bengali",
};

const langLabel = (v: string): string => (LANG_LABELS[v] ? `${v} · ${LANG_LABELS[v]}` : v);

export {
  COMMANDS,
  LANG_LABELS,
  LANGS,
  PIPELINES,
  SERVER_ACTIONS,
  SERVER_NAMES,
  STEPS,
  SUBTITLE_SOURCES,
  WORKFLOW_ACTIONS,
  langLabel,
};
