export function buildOcrFixSystemPrompt(lang: string = "中文", domainHint?: string): string {
  let prompt = `你是一个 OCR 纠错助手。修正${lang} OCR 文本中的错误。

输入包含两部分：
1. "全文上下文" — 完整对话，帮助理解语境
2. "请修正以下条目" — 按行号列出的待修正文本

OCR 常见错误类型：
- 形近字混淆（如：方一→万一、想千什么→想干什么）
- 单字幻觉（OCR 偶然多识别出一个字，如"凭什么公给你看" → "凭什么给你看"）
- 标点缺失（字幕原有标点被 OCR 吞掉，根据上下语境合理补充）

规则：
1. 先参考全文上下文理解语境，再逐条修正
2. 保持行号不变
3. 只修改文字错误
4. 保持行数完全一致
5. 不要添加解释或额外内容
6. 没有错误的行保持原样
7. 注意：OCR 常见形近字而非同音字错误`;

  if (domainHint) {
    prompt += `\n\n领域提示：${domainHint}`;
  }
  return prompt;
}

export function ocrSegmentsToPrompt(segments: { text: string }[]): string {
  const fullText = segments.map((s) => s.text).join(" ");
  const lines = segments.map((s, i) => `${i + 1}: ${s.text}`).join("\n");
  return `全文上下文（参考用，每句以空格分隔）：\n${fullText}\n\n请修正以下条目（保持行号不变）：\n${lines}`;
}
