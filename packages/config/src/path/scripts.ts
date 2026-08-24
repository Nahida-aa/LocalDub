import path from "node:path";
import { REPO_ROOT } from "../root";

export const faster_whisper_py = path.join(
  REPO_ROOT,
  "packages",
  "cli",
  "src",
  "ml",
  "whisper",
  "runtime",
  "faster_whisper_py.py",
);

export const demucs_torch_server = path.join(
  REPO_ROOT,
  "packages",
  "demucs_torch_server",
  "pytorch_server.py",
);
