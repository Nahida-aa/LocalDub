/**
 * translate.[dstLang].json 结构
 *
 * 由 translate 阶段写入，split_audio/tts/merge_audio/merge_video 读取。
 * 时间戳源自 {srt}.json（毫秒），文本是 LLM 翻译结果。
 * 此文件在此阶段后冻结，split_audio 不修改它，而是创建 timings.json。
 */
export interface TranslateFile {
  translation: {
    src: string;
    dst: string;
    src_lang: string;
    dst_lang: string;
    start_ms: number;
    end_ms: number;
    speaker: string;
  }[];
}
