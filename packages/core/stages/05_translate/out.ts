import { SubtitleSegment } from "@repo/subtitle/types";
import { TargetLang } from "../../const/lang";

export type TranslateSegment = SubtitleSegment & {
  dst: string; // (dubbed | sutitled) translation
  src_lang?: string;
  dst_lang?: TargetLang;
  speaker?: string; // 没有真正的消费, 也没有真的提供者, 因为不影响当前流程管道 的各阶段处理, 暂时作为占位
};

/**
 * translate.[dstLang].json 结构
 *
 * 由 translate 阶段写入，split_audio/tts/mix_audio/mix_video 读取。
 * 时间戳源自 {srt}.json（毫秒），文本是 LLM 翻译结果。
 * 此文件在此阶段后冻结，split_audio 不修改它，而是创建 timings.json。
 */
export interface TranslateResult {
  segments: TranslateSegment[];
  meta: TranslateResultMeta;
}

export type TranslateResultMeta = {
  src_lang: string;
  target_lang: TargetLang;
};
