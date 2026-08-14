import { z } from "zod/v4";
import { langList } from "../../const/lang";

export const TranslateCliInputSchema = z
  .looseObject({
    apiBase: z.string().optional(),
    model: z.string().optional(),
    targetLang: z
      .enum(langList)
      .optional()
      .describe("如果不填则 按照这个逻辑: 源语言: zh -> en, 否则 any -> zh"), //
    enabled: z.boolean().default(true).describe("设为 false 跳过翻译，直接使用原始识别文本"),
  })
  .prefault({});
