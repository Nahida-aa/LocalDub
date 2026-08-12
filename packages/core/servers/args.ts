import { serverTypeList } from "@repo/config/servers";
import { z } from "zod";

export const ServersArgsSchema = z
  .looseObject({
    action: z
      .enum(["status", "start", "stop", "discovery"])
      .default("status")
      .optional()
      .describe("服务器操作"),
    name: z.enum(serverTypeList).optional().describe("指定操作的服务器，不传则操作所有"),
  })
  .optional();
