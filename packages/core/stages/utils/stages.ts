import { StageName } from "../../tasks/args.ts";
import { readInputArgs } from "../../input/input.ts";

export const DUB_STAGES: StageName[] = [
  "separate",
  "separate_after",
  "asr",
  "asr_fix",
  "translate",
  "split_audio",
  "tts",
  "mix_audio",
  "mix_video",
];

export const DUB_SF_OCR_STAGES: StageName[] = [
  "separate",
  "separate_after",
  "sf_ocr_pre",
  "sf_ocr",
  "sf_ocr_fix",
  "translate",
  "split_audio",
  "tts",
  "mix_audio",
  "mix_video",
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
  "mix_audio",
  "mix_video",
];

export const SUBTITLE_STAGES: StageName[] = [
  "separate",
  "separate_after",
  "asr",
  "asr_fix",
  "translate",
  "split_audio",
  "mix_video",
];

/** Build stage list based on pipeline mode and subtitleSource config */
export function getStages(pipeline?: string): StageName[] {
  let stages = pipeline === "subtitle" ? SUBTITLE_STAGES : DUB_STAGES;
  try {
    const args = readInputArgs();
    const src = args.task.subtitleSource ?? "asr";
    if (src === "sf_ocr") stages = DUB_SF_OCR_STAGES;
    else if (src === "asr_ocr") stages = DUB_ASR_OCR_STAGES;
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
