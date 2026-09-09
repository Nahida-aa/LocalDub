import * as ort from "onnxruntime-node";
import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import { readFileSync, writeFileSync, rmSync, mkdtempSync, existsSync, readdirSync } from "node:fs";
import { Transformer, ResizeFit } from "@napi-rs/image";
// @ts-ignore - no types published
import { PNG } from "pngjs";
import { REPO_ROOT } from "@repo/config/root";
import { RAPIDOCR_MODEL_DIR } from "@repo/config/path/models";

export function findRapidOcrModelsDir(): string {
  const candidates: string[] = [];

  if (process.env.OCR_MODELS_DIR) {
    candidates.push(process.env.OCR_MODELS_DIR);
  }
  candidates.push(RAPIDOCR_MODEL_DIR);

  const modelsSub = "rapidocr_onnxruntime";
  const modelsPath = "models";
  if (process.platform === "win32") {
    const venvBase = resolve(REPO_ROOT, ".venv");
    candidates.push(join(venvBase, "Lib", "site-packages", modelsSub, modelsPath));
  } else {
    const libDir = resolve(REPO_ROOT, ".venv", "lib");
    if (existsSync(libDir)) {
      const entries = readdirSync(libDir, { withFileTypes: true });
      const pyDirs = entries
        .filter((e) => e.isDirectory() && /^python\d+\.\d+$/.test(e.name))
        .map((e) => e.name)
        .sort()
        .reverse();
      for (const pyDir of pyDirs) {
        candidates.push(join(libDir, pyDir, "site-packages", modelsSub, modelsPath));
      }
      candidates.push(join(libDir, "site-packages", modelsSub, modelsPath));
    }
  }

  for (const c of candidates) {
    if (existsSync(c)) return c;
  }

  throw new Error(
    `rapidocr models not found. Searched: ${candidates.join(", ")}\n` +
      `Hint: pip install rapidocr-onnxruntime in .venv, set OCR_MODELS_DIR env var, or ensure data/models/rapidocr/ exists.`,
  );
}

let _cached: string | undefined;

export function getRapidOcrModelsDir(): string {
  if (!_cached) _cached = findRapidOcrModelsDir();
  return _cached;
}
