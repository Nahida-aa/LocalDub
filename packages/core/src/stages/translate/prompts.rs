//! translate 阶段 prompt 构造 (镜像 TS `packages/core/stages/05_translate/utils.ts`)。

/// 视频元信息视图 (镜像 TS `MetaView`)。
#[derive(Debug, Clone, Default)]
pub struct MetaView {
    pub title: String,
    pub uploader: String,
    pub description: String,
}

/// 构造翻译系统提示 (镜像 TS `buildTranslateSystem`)。
pub fn build_translate_system(
    dst_lang_name: &str,
    src_lang_name: &str,
    meta: &MetaView,
    summary: &str,
    hotwords_str: &str,
    corrections_str: &str,
) -> String {
    format!(
        "你是一个专业的{dst_lang_name}翻译助手。请将{src_lang_name}逐句翻译成{dst_lang_name}。\n\n\
# 元信息\n\
视频标题：{title}\n\
作者：{uploader}\n\
描述：{description}\n\
摘要：{summary}\n\n\
# 翻译热词\n\
{hotwords}\n\n\
# ASR 纠错\n\
{corrections}\n\n\
# 规则\n\
1) 准确自然。忠实传达原意，口语保持口语感，书面保持克制；避免直译腔与过度文学化；不擅自增删信息。\n\
2) 逐句对齐。一句对一句。\n\
3) 人名、地名、品牌、型号、缩写默认保留；文件名、路径、URL 一律保留原样。\n\
4) 使用{dst_lang_name}标点；破折号禁用，改用逗号或括号。\n\
5) 输出格式：{{\"dst\": [\"<对应{dst_lang_name}译文>\", \"<对应{dst_lang_name}译文>\", ...]}}\n\n\
用户消息会发送一个编号列表，请严格按顺序逐句翻译，每句一条。",
        dst_lang_name = dst_lang_name,
        src_lang_name = src_lang_name,
        title = meta.title,
        uploader = meta.uploader,
        description = meta.description,
        summary = if summary.is_empty() {
            "(none)"
        } else {
            summary
        },
        hotwords = hotwords_str,
        corrections = corrections_str,
    )
}

/// 构造预处理 prompt (镜像 TS `buildPreprocessPrompt`)。
pub fn build_preprocess_prompt(
    dst_lang_name: &str,
    src_lang_name: &str,
    meta: &MetaView,
    full_text: &str,
) -> String {
    format!(
        "你为视频字幕翻译做预处理。请阅读视频元信息和完整转录文本，输出 JSON。\n\
转录原始语言：{src_lang_name}\n\
目标译文语言：{dst_lang_name}\n\n\
# 输出 JSON 格式（严格遵守）\n\
{{\n\
\"summary\": \"<中文写的视频摘要，3-5 句>\",\n\
\"hotwords\": [\n\
  {{\"src\": \"<原文术语>\", \"dst\": \"<目标语言推荐译法>\"}}\n\
],\n\
\"corrections\": [\n\
  {{\"wrong\": \"<转录中明显错认的写法>\", \"correct\": \"<正确写法>\">}}\n\
]\n\
}}\n\n\
# 视频元信息\n\
标题：{title}\n\
作者：{uploader}\n\
描述：{description}\n\n\
# 转录文本\n\
{full}",
        dst_lang_name = dst_lang_name,
        src_lang_name = src_lang_name,
        title = meta.title,
        uploader = meta.uploader,
        description = meta.description,
        full = if full_text.len() > 10000 {
            &full_text[..10000]
        } else {
            full_text
        },
    )
}
