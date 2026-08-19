import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { env } from "@repo/config/env";
import { timeId } from "../shared/db/timeId.ts";
import { CookieArgsSchema } from "@repo/core/cmd/cookie/args";
import { readInputArgs } from "@repo/core/input/input";

import { cmdCheck } from "./src/feat/command/check.ts";
import { cmdServers } from "./src/feat/command/servers.ts";

const input = readInputArgs();
const cmd = input.command;

if (
  input.task.pipeline === "subtitle" &&
  input.stages?.separate?.always &&
  !input.stages?.asr?.useSeparated
) {
  console.warn(
    "[WARN] subtitle 模式下 separate.always=true 但 ASR 未使用分离人声 (asr.useSeparated=false)，separate 跑完不会被 ASR 使用",
  );
}

switch (cmd) {
  case "check": {
    const p = input.check;

    await cmdCheck({ type: p?.type, taskDir: p?.taskDir ?? undefined });
    break;
  }

  case "deviceInfo": {
    const { getDeviceInfo } = await import("@repo/device");
    const info = await getDeviceInfo();
    console.log(JSON.stringify(info, null, 2));
    break;
  }

  case "listModels": {
    const apiBase =
      input.stages?.translate?.apiBase || env.OPENAI_BASE_URL || "https://api.openai.com/v1";
    const apiKey = env.OPENAI_API_KEY;
    if (!apiKey) {
      console.error("OPENAI_API_KEY not configured");
      process.exit(1);
    }
    try {
      const resp = await fetch(`${apiBase}/models`, {
        headers: { Authorization: `Bearer ${apiKey}` },
      });
      if (!resp.ok) throw new Error(`API ${resp.status}: ${await resp.text()}`);
      const data = (await resp.json()) as any;
      for (const m of data.data || []) {
        console.log(`  ${m.id}`);
      }
    } catch (err) {
      console.error("listModels failed:", err);
      process.exit(1);
    }
    break;
  }
  case "servers": {
    cmdServers(input);
    break;
  }
  case "task": {
    const { cmdTask } = await import("@repo/core/cmd/tasks/task");
    await cmdTask(input);
    break;
  }
  case "env": {
    const envArgs = input.env ?? { action: "check", targets: [] };
    const { runCheck, runEnsure, formatResult } = await import("@repo/core/cmd/env/index");
    const results =
      envArgs.action === "ensure"
        ? await runEnsure(envArgs.targets)
        : await runCheck(envArgs.targets);
    for (const r of results) {
      console.log(formatResult(r));
    }
    break;
  }
  case "cookie": {
    const { cmdCookie } = await import("@repo/core/cmd/cookie/handler");
    await cmdCookie(input.cookie ?? CookieArgsSchema.parse({}));
    break;
  }
  default: {
  }
}
