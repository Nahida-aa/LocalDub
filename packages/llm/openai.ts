import { env } from "@repo/config/env";

export const chat_completions = async (
  prompt: string,
  opts: {
    model?: string;
    apiBase?: string;
    systemPrompt: string;
    signal?: AbortSignal;
    max_tokens?: number;
    api_key?: string;
    temperature?: number;
  },
) => {
  const apiBase = opts?.apiBase || env.OPENAI_BASE_URL;
  const api_key = opts?.api_key || env.OPENAI_API_KEY;

  const model = opts?.model || env.OPENAI_MODEL;
  const max_tokens = opts?.max_tokens ?? 4096;
  const temperature = opts?.temperature ?? 0.1;
  const resp = await fetch(`${apiBase}/chat/completions`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${api_key}`,
    },
    signal: opts?.signal,
    body: JSON.stringify({
      model,
      max_tokens,
      temperature,
      messages: [
        { role: "system", content: opts.systemPrompt },
        { role: "user", content: prompt },
      ],
    }),
  });
  if (!resp.ok) {
    const err = await resp.text();
    throw new Error(`LLM API ${resp.status}: ${err}`);
  }
  const json = await resp.json();
  return (json.choices?.[0]?.message?.content || "").trim();
};
