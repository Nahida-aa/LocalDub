import { z } from "zod";

export const AsrOcrPreArgsSchema = z.looseObject({
  fps: z.number().default(2).describe("帧率 (fps), 越高时间戳越准但越慢; 默认 2"),
});
