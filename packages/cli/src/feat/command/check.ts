import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { readInputArgs } from "@repo/core/input/input";

export async function cmdCheck(opts: {
  type: "video" | "asr" | "font" | undefined;
  taskDir?: string;
}) {
  const { type, taskDir } = opts;

  if (type === "video") {
    if (!taskDir) {
      console.log(JSON.stringify({ ok: false, error: "check video requires taskDir" }));
      process.exit(1);
    }

    const videoPath = join(taskDir, "media", "video_source.mp4");
    if (!existsSync(videoPath)) {
      console.log(JSON.stringify({ ok: false, error: "video_source.mp4 not found" }));
      process.exit(1);
    }
    const stat = statSync(videoPath);
    console.log(
      JSON.stringify({
        ok: true,
        type: "video",
        path: videoPath,
        size: stat.size,
      }),
    );
    return;
  }

  if (type === "asr") {
    if (!taskDir) {
      console.log(JSON.stringify({ ok: false, error: "check asr requires taskDir" }));
      process.exit(1);
    }

    const asrPath = join(taskDir, "asr_fix", "asr_fix.json");
    const asrRawPath = join(taskDir, "asr", "asr.json");
    let asrFile = asrPath;
    if (!existsSync(asrPath) && existsSync(asrRawPath)) asrFile = asrRawPath;
    if (!existsSync(asrFile)) {
      console.log(JSON.stringify({ ok: false, error: "asr.json not found" }));
      process.exit(1);
    }
    const asr = JSON.parse(readFileSync(asrFile, "utf-8"));
    const segments = asr.result?.segments ?? [];
    const audioDurationMs = asr.meta?.audio_duration ?? 0;
    const total = segments.length;
    const timeline: Record<string, unknown>[] = [];
    const issues: Record<string, unknown>[] = [];
    let zeroGaps = 0;

    for (let i = 0; i < total; i++) {
      const s = segments[i] as Record<string, any>;
      const entry: Record<string, unknown> = {
        idx: i + 1,
        text: (s.text ?? "").slice(0, 60),
        startMs: Math.round(s.start_ms),
        endMs: Math.round(s.end_ms),
      };
      const gapMs =
        i > 0 ? Math.round(s.start_ms - (segments[i - 1] as Record<string, any>).end_ms) : 0;
      entry.gapMs = gapMs;
      if (gapMs === 0) zeroGaps++;
      const warnings: string[] = [];
      if (i > 0 && gapMs === 0) warnings.push("start 紧跟上段结束");
      if (
        Math.round(s.end_ms) === audioDurationMs ||
        (i < total - 1 &&
          Math.round(s.end_ms) === Math.round((segments[i + 1] as Record<string, any>).start_ms))
      )
        warnings.push("end 拉到分段边界");
      const durationMs = Math.round(s.end_ms - s.start_ms);
      if (durationMs > 5000) warnings.push(`时长 ${(durationMs / 1000).toFixed(1)}s 超过 5s`);
      if (warnings.length > 0) entry.warnings = warnings;
      timeline.push(entry);
    }

    if (total > 1 && zeroGaps === total - 1) {
      issues.push({
        type: "vad_not_fired",
        detail: `全部 ${total - 1} 个段间间隙为 0ms，VAD 可能未生效`,
        suggestion: "检查 ASR 引擎的 VAD 配置",
      });
    } else if (zeroGaps > 0) {
      issues.push({
        type: "partial_zero_gaps",
        detail: `${zeroGaps}/${total - 1} 个段间间隙为 0ms`,
      });
    }

    const engine = asr.meta?.engine ?? "unknown";
    const result: Record<string, unknown> = {
      ok: true,
      type: "asr",
      engine,
      audioDurationMs,
      segments: total,
      zeroGaps,
      timeline,
    };
    if (issues.length > 0) result.issues = issues;
    console.log(JSON.stringify(result, null, 2));
    return;
  }

  if (type === "font") {
    const cfg = readInputArgs();
    const configuredFont = cfg.stages?.merge_video?.font ?? "Noto Sans CJK SC";

    const result: Record<string, unknown> = {
      ok: true,
      type: "font",
      configured: configuredFont,
    };

    if (process.platform === "win32") {
      result.available = true;
      result.cjkFonts = ["Microsoft YaHei", "SimHei", "SimSun"];
      result.note = "Windows 字体检测暂不支持 fc-list，使用已知 CRT 字体列表";
    } else {
      const fcRaw = (cmd: string, args: string[]): string => {
        const r = spawnSync(cmd, args, { timeout: 5000, encoding: "utf-8" });
        return r.status === 0 ? r.stdout.trim() : "";
      };

      const cjkRaw = fcRaw("fc-list", [":lang=zh", "family"]);
      const cjkFonts = [
        ...new Set(
          cjkRaw
            .split("\n")
            .map((l) => l.trim())
            .filter(Boolean)
            .flatMap((l) => l.split(",").map((s) => s.trim())),
        ),
      ].sort();

      const matchRaw = fcRaw("fc-list", [`:family=${configuredFont}`]);
      const available = matchRaw.length > 0;

      result.available = available;
      result.cjkFonts = cjkFonts;
      if (!available) {
        result.suggestion =
          cjkFonts.length > 0
            ? `字体 "${configuredFont}" 未安装，可用 CJK 字体：${cjkFonts.join("、")}`
            : `字体 "${configuredFont}" 未安装，可尝试：sudo apt install fonts-noto-cjk`;
      }
    }

    console.log(JSON.stringify(result, null, 2));
    return;
  }

  console.log(JSON.stringify({ ok: false, error: `unknown check type: ${type}` }));
  process.exit(1);
}
