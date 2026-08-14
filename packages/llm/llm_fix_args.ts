import { z } from "zod";

const LlmArgsSchema = z.object({
  llmModel: z.string().default("gemma4:31b-cloud").describe("LLM 模型名"),
  llmApiBase: z.string().default("http://localhost:11434/v1").describe("LLM API 地址"),
  domainHint: z
    .string()
    .optional()
    .describe('领域提示, 帮助 LLM 理解上下文，例如"仙侠题材，角色：叶白、慧天、夜白"'),
});
export type LlmArgs = z.output<typeof LlmArgsSchema>;

export const LlmFixArgsSchema = LlmArgsSchema.extend({
  llmFix: z.boolean().default(false).describe("是否启用 LLM 修正"),
});
export type LlmFixArgs = z.output<typeof LlmFixArgsSchema>;
