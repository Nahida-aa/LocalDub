import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { pythonBin } from "@repo/config/path/bin";
import { REPO_ROOT } from "@repo/config/root";
import { OCRLine } from "@repo/subtitle-ocr/types";

const PY_SCRIPT = resolve(REPO_ROOT, "packages", "subtitle-ocr", "subtitle-py.py");

export async function ocrFramePy(
  framePath: string,
  opts?: { textScore?: number; subtitleOnly?: boolean; device?: string; yRange?: [number, number] },
): Promise<OCRLine[]> {
  if (!existsSync(PY_SCRIPT)) {
    throw new Error(`Python OCR script not found: ${PY_SCRIPT}`);
  }

  const pyBin = process.env.LOCALDUB_OCR_PYTHON || pythonBin();
  const args: string[] = [PY_SCRIPT, framePath];
  if (opts?.textScore != null) {
    args.push("--text-score", String(opts.textScore));
  }
  if (opts?.subtitleOnly) {
    args.push("--subtitle-only");
  }
  if (opts?.device && opts.device !== "cpu") {
    args.push("--device", opts.device);
  }
  if (opts?.yRange) {
    args.push("--y-range", String(opts.yRange[0]), String(opts.yRange[1]));
  }

  const r = spawnSync(pyBin, args, {
    timeout: 60_000,
    encoding: "utf-8",
  });

  if (r.status !== 0) {
    throw new Error(`subtitle-py failed (exit ${r.status}): ${(r.stderr || "").slice(-300)}`);
  }

  const parsed = JSON.parse(r.stdout);

  const lines: OCRLine[] = [];
  for (const seg of parsed.lines || []) {
    lines.push({
      text: seg.text,
      confidence: seg.confidence,
      box: seg.box || [],
    });
  }
  return lines;
}
