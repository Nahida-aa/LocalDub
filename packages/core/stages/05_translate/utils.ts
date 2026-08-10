import { TargetLang } from "../../cmd/tasks/input";
import { setCtx, TaskCtx } from "../../context/context";
import { readTaskLanguages } from "../utils/utils";

export interface MetaView {
  title: string;
  uploader: string;
  description: string;
}

export const buildTranslateSystem = ({
  dstLangName,
  srcLangName,
  metaView,
  summary,
  hotwordsStr,
  correctionsStr,
}: {
  dstLangName: string;
  srcLangName: string;
  metaView: MetaView;
  summary: string | undefined;
  hotwordsStr: string;
  correctionsStr: string;
}) => `你是一个专业的${dstLangName}翻译助手。请将${srcLangName}逐句翻译成${dstLangName}。

# 元信息
视频标题：${metaView.title}
作者：${metaView.uploader}
描述：${metaView.description}
摘要：${summary || "(none)"}

# 翻译热词
${hotwordsStr}

# ASR 纠错
${correctionsStr}

# 规则
1) 准确自然。忠实传达原意，口语保持口语感，书面保持克制；避免直译腔与过度文学化；不擅自增删信息。
2) 逐句对齐。一句对一句。
3) 人名、地名、品牌、型号、缩写默认保留；文件名、路径、URL 一律保留原样。
4) 使用${dstLangName}标点；破折号禁用，改用逗号或括号。
5) 输出格式：{"dst": ["<对应${dstLangName}译文>", "<对应${dstLangName}译文>", ...]}

用户消息会发送一个编号列表，请严格按顺序逐句翻译，每句一条。`;

export const buildPreprocessPrompt = ({
  dstLangName,
  srcLangName,
  metaView,
  fullText,
}: {
  dstLangName: string;
  srcLangName: string;
  metaView: MetaView;
  fullText: string;
}) => `你为视频字幕翻译做预处理。请阅读视频元信息和完整转录文本，输出 JSON。
转录原始语言：${srcLangName}
目标译文语言：${dstLangName}

# 输出 JSON 格式（严格遵守）
{
"summary": "<中文写的视频摘要，3-5 句>",
"hotwords": [
  {"src": "<原文术语>", "dst": "<目标语言推荐译法>"}
],
"corrections": [
  {"wrong": "<转录中明显错认的写法>", "correct": "<正确写法>"}
]
}

# 视频元信息
标题：${metaView.title}
作者：${metaView.uploader}
描述：${metaView.description}

# 转录文本
${fullText.slice(0, 10000)}`;

/*
 * 解析目标语言: input > auto 推断, 由翻译步骤调用, 此时 ctx 中不存在目标语言
 */
export function resolveLanguage(ctx: TaskCtx) {
  // 解析目标语言: input > auto 推断
  const input_target_lang = ctx.input.stages?.translate?.targetLang;
  const { asrLanguage: srcLang = "zh", targetLanguage: existingDstLang } = readTaskLanguages(ctx);
  const resolvedDstLang = input_target_lang ?? (srcLang === "zh" ? "en" : "zh");

  if (resolvedDstLang !== existingDstLang) {
    setCtx(ctx.task.task_dir, { target_language: resolvedDstLang });
  }
  return {
    targetLang: resolvedDstLang,
    srcLang,
  };
}
