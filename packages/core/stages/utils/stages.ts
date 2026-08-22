import { StageName } from "@repo/core/cmd/tasks/input";
import { readInputArgs } from "../../input/input.ts";

export const DUB_STAGES: StageName[] = [
  "separate",
  "separate_after",
  "asr",
  "asr_fix",
  "translate",
  "split_audio",
  "tts",
  "merge_audio",
  "merge_video",
];

export const DUB_ASR_OCR_STAGES: StageName[] = [
  "separate",
  "separate_after",
  "asr",
  "asr_ocr_pre",
  "asr_ocr",
  "asr_ocr_fix",
  "translate",
  "split_audio",
  "tts",
  "merge_audio",
  "merge_video",
];

export const SUBTITLE_STAGES: StageName[] = [
  "separate",
  "separate_after",
  "asr",
  "asr_fix",
  "translate",
  "split_audio",
  "merge_video",
];

function withOcrStages(stages: StageName[], pipeline?: string): StageName[] {
  const drop = new Set(["asr", "asr_fix", "separate", "separate_after"]);
  if (pipeline === "subtitle") drop.add("separate");
  const filtered = stages.filter((s) => !drop.has(s));
  const idx = filtered.findIndex((s) => s === "translate");
  const out = [...filtered];
  const ocrStages: StageName[] = ["ocr", "ocr_fix"];
  if (idx === -1) {
    out.push(...ocrStages);
  } else {
    out.splice(idx, 0, ...ocrStages);
  }
  return out;
}

/**
 * subtitleSource === 'file' 时的阶段列表：跳过 asr/asr_fix（字幕来自外部文件），
 * 在 translate 前插入 import_subtitle 阶段；pipeline === 'subtitle' 或
 * stages.import_subtitle.skipSeparate 时去掉 separate，skipSeparate 时额外去掉 separate_after。
 */
function withFileSubtitleStages(
  stages: StageName[],
  pipeline?: string,
  skipSeparate?: boolean,
): StageName[] {
  const drop = new Set<StageName>(["asr", "asr_fix"]);
  if (pipeline === "subtitle" || skipSeparate) drop.add("separate");
  if (skipSeparate) drop.add("separate_after");
  const filtered = stages.filter((s) => !drop.has(s));
  const idx = filtered.findIndex((s) => s === "translate");
  const out = [...filtered];
  if (idx === -1) {
    out.push("import_subtitle");
  } else {
    out.splice(idx, 0, "import_subtitle");
  }
  return out;
}

// function withAsrOcrStages(stages: StageName[], _pipeline?: string): StageName[] {
// 	const out: StageName[] = [];
// 	for (const s of stages) {
// 		if (s === 'asr_fix' || s === 'ocr' || s === 'ocr_fix') continue;
// 		out.push(s);
// 		if (s === 'asr') {
// 			out.push('asr_ocr_pre',);
// 			out.push('asr_ocr',);
// 			out.push('asr_ocr_fix',);
// 		}
// 	}
// 	return out;
// }

/** Build stage list based on pipeline mode and subtitleSource config */
export function getStages(pipeline?: string): StageName[] {
  let stages = pipeline === "subtitle" ? SUBTITLE_STAGES : DUB_STAGES;
  try {
    const args = readInputArgs();
    const src = args.task.subtitleSource ?? "asr";
    if (src === "ocr") stages = withOcrStages(stages, pipeline);
    else if (src === "asr_ocr") stages = DUB_ASR_OCR_STAGES;
    else if (src === "file")
      stages = withFileSubtitleStages(stages, pipeline, args.stages?.import_subtitle?.skipSeparate);
    if (args.stages?.translate?.enabled === false) {
      stages = stages.filter((s) => s !== "translate");
    }
    if (pipeline === "subtitle" && args.stages?.split_audio?.vadAlign !== true) {
      stages = stages.filter((s) => s !== "split_audio");
    }
  } catch {
    // config may not be available (e.g. import time); use default
  }
  return stages;
}
