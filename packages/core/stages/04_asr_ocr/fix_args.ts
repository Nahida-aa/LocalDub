import { z } from "zod";
import { OcrFixArgsSchema } from "../sf_ocr/fix_args";

export const AsrOcrFixArgsSchema = z
  .looseObject({
    ...OcrFixArgsSchema.shape,
    is_resample: z.boolean().default(false),
  })
  .prefault({});

export type AsrOcrFixArgs = z.output<typeof AsrOcrFixArgsSchema>;
