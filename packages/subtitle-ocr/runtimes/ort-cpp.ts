import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { basename, resolve } from "node:path";
import { REPO_ROOT } from "@repo/config/root";
import { OcrBoxResult } from "@repo/subtitle-ocr/types";
import { getRapidOcrModelsDir } from "@repo/subtitle-ocr/utils";
import {
  existsOcrCppBin,
  ocrCppBinPath,
  checkOcrCppBin,
  ensureOcrCppBin,
} from "@repo/env/items/ocr_cpp_bin";

const BUILD_DIR = resolve(REPO_ROOT, "packages", "subtitle-ocr", "ort-cpp", "build");
const OCR_KEYS_PATH = resolve(REPO_ROOT, "packages", "subtitle-ocr", "ppocr_keys.json");
function getLibPathKey(): string {
  if (process.platform !== "win32") return "LD_LIBRARY_PATH";
  const existing = Object.keys(process.env).find((k) => k.toLowerCase() === "path");
  return existing || "PATH";
}
const LIB_PATH_KEY = getLibPathKey();

let _ocrChecked = false;

async function ensureOcrCpp(): Promise<void> {
  if (_ocrChecked) return;
  _ocrChecked = true;
  if (existsOcrCppBin()) {
    const c = await checkOcrCppBin();
    if (c.status === "pass") return;
  }
  const r = await ensureOcrCppBin();
  if (r.status !== "pass") throw new Error(`OCR rebuild failed: ${JSON.stringify(r.data)}`);
}

export async function ocrFrameOpenCvCpp(
  framePath: string,
  opts?: { textScore?: number; subtitleOnly?: boolean; device?: string },
): Promise<OcrBoxResult[]> {
  await ensureOcrCpp();
  const r = spawnSync(
    ocrCppBinPath(),
    [
      framePath,
      ...(opts?.textScore != null ? [String(opts.textScore)] : []),
      ...(opts?.subtitleOnly ? ["--subtitle-only"] : []),
      ...(opts?.device && opts.device !== "cpu" ? ["--device", opts.device] : []),
    ],
    {
      timeout: 60_000,
      encoding: "utf-8",
      env: ocrEnv(),
    },
  );

  if (r.status !== 0) {
    throw new Error(
      `subtitle_ocr_ort_cpp failed (exit ${r.status}): ${(r.stderr || "").slice(-300)}`,
    );
  }

  const parsed = parseBatchOutput(r.stdout);
  const filename = basename(framePath);
  return parsed.get(filename) || [];
}

export async function ocrFramesOpenCvCpp(
  frameDir: string,
  opts?: { textScore?: number; subtitleOnly?: boolean; device?: string },
): Promise<Map<string, OcrBoxResult[]>> {
  await ensureOcrCpp();
  const binPath = ocrCppBinPath();
  const args = [
    "--dir",
    frameDir,
    ...(opts?.textScore != null ? [String(opts.textScore)] : []),
    ...(opts?.subtitleOnly ? ["--subtitle-only"] : []),
    ...(opts?.device && opts.device !== "cpu" ? ["--device", opts.device] : []),
  ];
  const env = ocrEnv();
  const r = spawnSync(binPath, args, {
    encoding: "utf-8",
    env,
  });

  if (r.status !== 0) {
    const stderr = (r.stderr || "").slice(-500);
    const stdout = (r.stdout || "").slice(-500);
    const diag = [
      `exit=${r.status} signal=${r.signal}`,
      `bin=${binPath}`,
      `frameDir=${frameDir}`,

      `ortLib exists=${existsSync(ORT_LIB_DIR)}`,
      `buildDir exists=${existsSync(BUILD_DIR)}`,
      stderr && `stderr:\n${stderr}`,
      stdout && `stdout:\n${stdout}`,
    ]
      .filter(Boolean)
      .join("\n");
    throw new Error(`subtitle_ocr_ort_cpp --dir failed\n${diag}`);
  }

  return parseBatchOutput(r.stdout);
}

/** Directory containing onnxruntime.dll + MinGW runtime DLLs (libstdc++-6.dll, etc.) */
const ORT_LIB_DIR = resolve(
  REPO_ROOT,
  "packages",
  "tmp",
  "onnxruntime-win-x64-1.26.0",
  "onnxruntime-win-x64-1.26.0",
  "lib",
);

function ocrEnv(): Record<string, string | undefined> {
  const extra: string[] = [];
  if (process.platform === "win32" && existsSync(ORT_LIB_DIR)) extra.push(ORT_LIB_DIR);
  const libPath = [...extra, BUILD_DIR, process.env[LIB_PATH_KEY] || ""].filter(Boolean).join(";");
  return {
    ...process.env,
    [LIB_PATH_KEY]: libPath,
    OCR_MODELS_DIR: getRapidOcrModelsDir(),
    OCR_KEYS_PATH,
  };
}

function parseBatchOutput(stdout: string): Map<string, OcrBoxResult[]> {
  const items: any[] = JSON.parse(stdout);
  const result = new Map<string, OcrBoxResult[]>();
  for (const item of items) {
    const lines: OcrBoxResult[] = [];
    for (const seg of item.segments || []) {
      lines.push({
        text: seg.text,
        text_confidence: seg.confidence,
        box: seg.box || [],
        x_range: [0, 0],
        y_range: [0, 0],
        center: [0, 0],
      });
    }
    if (lines.length === 0 && item.text) {
      lines.push({
        text: item.text,
        text_confidence: 1,
        box: [],
        x_range: [0, 0],
        y_range: [0, 0],
        center: [0, 0],
      });
    }
    result.set(item.file || "", lines);
  }
  return result;
}
