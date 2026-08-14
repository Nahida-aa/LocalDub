import { z } from "zod";

const alignmentList = [
  "bottom-left",
  "bottom-center",
  "bottom-right",
  "middle-left",
  "center",
  "middle-right",
  "top-left",
  "top-center",
  "top-right",
] as const;
type Alignment = (typeof alignmentList)[number];
const AlignmentSchema = z.enum(alignmentList).default("bottom-center");

const ALIGNMENT_MAP: Record<Alignment, number> = Object.fromEntries(
  alignmentList.map((key, i) => [key, i + 1]),
) as Record<Alignment, number>;

export function alignmentToFfmpeg(alignment: Alignment): number {
  return ALIGNMENT_MAP[alignment] ?? 2;
}

export const MergeVideoArgsSchema = z
  .object({
    fontSize: z
      .number()
      .min(1)
      .max(200)
      .nullish()
      .describe("字幕字号，不填则自动: 竖屏: 12(zh) / 9(其他) ← 横屏: 24(zh) / 18(其他)"),
    marginV: z.number().min(0).nullish().describe("垂直边距(像素)，不填则自动: 竖屏 70 / 横屏 5"),
    alignment: AlignmentSchema.optional(),
    outline: z.number().min(0).default(0).optional(),
    shadow: z.number().min(0).default(1).optional(),
    font: z.string().optional().describe("ASS 字幕字体名（须系统已安装），默认 Noto Sans CJK SC"),
    srtPath: z.string().optional().describe("调试使用"),
    bgmPath: z.string().optional().describe("调试使用"),
    bgmGain: z.number().default(-6).optional().describe("背景音乐增益(dB), 0=不变, 负值=衰减"),
    dubGain: z.number().default(3).optional().describe("配音增益(dB), 补偿合成语音偏小的听感差"),
  })
  .default({
    alignment: "bottom-center",
    outline: 0,
    shadow: 1,
    bgmGain: -6,
    dubGain: 3,
  })
  .optional();
