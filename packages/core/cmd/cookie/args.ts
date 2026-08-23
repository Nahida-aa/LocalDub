import { z } from "zod";

export const CookieArgsSchema = z.object({
  action: z.enum(["set"]).default("set"),
  service: z.enum(["youtube"]).default("youtube"),
  content: z.string().optional().describe("Netscape cookie，为空则让用户手动粘贴"),
});
export type CookieArgs = z.output<typeof CookieArgsSchema>;
