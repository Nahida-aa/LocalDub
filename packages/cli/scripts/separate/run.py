"""
CLI wrapper for Demucs PyTorch separation, callable from TypeScript via spawnSync.

Usage:
    .venv/bin/python packages/cli/scripts/separate/run.py <video_path> <session_path> [--device cpu|cuda]
    .venv/bin/python packages/cli/scripts/separate/run.py --benchmark-load [--device cpu|cuda]

Sets DEMUCS_DEVICE env var before loading the backend, so resolve_device() picks it up.

Writes audio_vocals.wav and audio_bgm.wav to <session_path>/media/.
Prints progress lines: [PROGRESS] <percent>
Prints on success: <vocals_path>\n<bgm_path>
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

if "--device" in sys.argv:
    idx = sys.argv.index("--device")
    if idx + 1 < len(sys.argv):
        os.environ["DEMUCS_DEVICE"] = sys.argv[idx + 1]

from _engine import load_separator, separate_audio


def main() -> None:
    if "--benchmark-load" in sys.argv:
        shifts = 3
        if "--shifts" in sys.argv:
            idx = sys.argv.index("--shifts")
            if idx + 1 < len(sys.argv):
                shifts = int(sys.argv[idx + 1])
        load_separator(shifts=shifts)
        print("[BENCHMARK_LOAD_DONE]")
        return

    args = [a for a in sys.argv[1:] if not a.startswith("--")]

    if len(args) < 2:
        print(f"Usage: {sys.argv[0]} <video_path> <session_path> [--device cpu|cuda] [--shifts N]", file=sys.stderr)
        sys.exit(1)

    video_file = Path(args[0])
    session = Path(args[1])

    if not video_file.is_file():
        print(f"Error: video file not found: {video_file}", file=sys.stderr)
        sys.exit(1)

    shifts = 3
    if "--shifts" in sys.argv:
        idx = sys.argv.index("--shifts")
        if idx + 1 < len(sys.argv):
            shifts = int(sys.argv[idx + 1])

    def progress_callback(progress: int, message: str) -> None:
        print(f"[PROGRESS] {progress}")

    try:
        vocals_file, bgm_file = separate_audio(video_file, session, progress_callback, shifts=shifts)
    except Exception as exc:
        print(f"Demucs separation failed: {exc}", file=sys.stderr)
        sys.exit(1)

    print(str(vocals_file))
    print(str(bgm_file))


if __name__ == "__main__":
    main()
