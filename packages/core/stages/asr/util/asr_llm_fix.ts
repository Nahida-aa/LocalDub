import { env } from "@repo/config/env";

export function buildAsrFixSystemPrompt(lang: string = "中文", domainHint?: string): string {
  let prompt = `你是一个 ASR 纠错助手。修正${lang}转录文本中的错别字。

输入包含两部分：
1. "全文上下文" — 完整对话，帮助理解语境
2. "请修正以下条目" — 按行号列出的待修正文本

规则：
1. 先参考全文上下文理解语境，再逐条修正
2. 保持行号不变
3. 只修改文字错误，不改标点
4. 保持行数完全一致
5. 不要添加解释或额外内容
6. 没有错误的行保持原样
7. 注意：中文 ASR 常见同音/近音字错误，根据上下文判断正确用词`;

  if (domainHint) {
    prompt += `\n\n领域提示：${domainHint}`;
  }
  return prompt;
}

export function segmentsToPrompt(segments: { text: string }[]): string {
  const fullText = segments.map((s) => s.text).join(" ");
  const lines = segments.map((s, i) => `${i + 1}: ${s.text}`).join("\n");
  return `全文上下文（参考用，每句以空格分隔）：\n${fullText}\n\n请修正以下条目（保持行号不变）：\n${lines}`;
}
