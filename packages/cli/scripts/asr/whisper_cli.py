"""
CLI ASR using whisper-cli (whisper.cpp compiled with HIPBLAS GPU support).

Usage:
    .venv/bin/python packages/cli/scripts/asr/whisper_cli.py <audio> <session_path> [language] [--gpu] [--n-threads 4]
    .venv/bin/python packages/cli/scripts/asr/whisper_cli.py --benchmark-load [--gpu]

Subprocess wrapper around submodule/whisper.cpp/build/bin/whisper-cli.
GPU is used by default (HIPBLAS). Pass --no-gpu for CPU.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
WHISPER_CLI = REPO_ROOT / "submodule" / "whisper.cpp" / "build" / "bin" / "whisper-cli"


def _parse_timing(output: str, key: str) -> float:
    m = re.search(rf"whisper_print_timings:\s+{key}\s+=\s+([\d.]+)\s+ms", output)
    return float(m.group(1)) / 1000 if m else 0.0


def _parse_segments(output: str) -> list[dict]:
    segments = []
    for line in output.split("\n"):
        m = re.match(r"^\[(\d+):(\d+):(\d+)\.(\d+)\s*-->\s*(\d+):(\d+):(\d+)\.(\d+)\]\s+(.*)", line)
        if m:
            start_s = int(m.group(1)) * 3600 + int(m.group(2)) * 60 + int(m.group(3)) + int(m.group(4)) / 100
            end_s = int(m.group(5)) * 3600 + int(m.group(6)) * 60 + int(m.group(7)) + int(m.group(8)) / 100
            segments.append({
                "text": m.group(9).strip(),
                "start": start_s,
                "end": end_s,
                "words": [],
            })
    return segments


def main() -> None:
    force_cpu = "--no-gpu" in sys.argv
    if "--benchmark-load" in sys.argv:
        # Generate 1s silent WAV
        silent_wav = Path("/tmp") / "whisper_cli_silent.wav"
        if not silent_wav.exists():
            subprocess.run(
                ["ffmpeg", "-y", "-f", "lavfi", "-i", "anullsrc=r=16000:cl=mono", "-t", "1",
                 "-c:a", "pcm_s16le", str(silent_wav)],
                capture_output=True, check=True,
            )
        model_path = os.environ.get("WHISPER_MODEL") or str(Path.home() / ".cache" / "pywhispercpp" / "ggml-large-v3-turbo.bin")
        result = subprocess.run(
            [str(WHISPER_CLI), "-m", model_path, str(silent_wav), "-l", "en", "-t", "4",
             *(["-ng"] if force_cpu else [])],
            capture_output=True, text=True, timeout=120,
        )
        load_time = _parse_timing(result.stderr, "load time") or _parse_timing(result.stdout, "load time")
        print(f"[BENCHMARK_LOAD_DONE] load_time={load_time:.3f}s")
        return

    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) < 2:
        print(f"Usage: {sys.argv[0]} <audio> <session_path> [language] [--no-gpu]", file=sys.stderr)
        sys.exit(1)

    audio_file = Path(args[0])
    session_path = Path(args[1])
    n_threads = 4

    if not audio_file.is_file():
        print(f"Error: audio file not found: {audio_file}", file=sys.stderr)
        sys.exit(1)

    # Extract audio to WAV (whisper-cli doesn't support MP4 natively)
    wav_path = audio_file.with_suffix(".wav")
    if audio_file.suffix != ".wav":
        wav_path = session_path / "audio_temp.wav"
        subprocess.run(
            ["ffmpeg", "-y", "-i", str(audio_file), "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", str(wav_path)],
            capture_output=True, check=True,
        )

    model_path = os.environ.get("WHISPER_MODEL") or str(Path.home() / ".cache" / "pywhispercpp" / "ggml-large-v3-turbo.bin")

    cmd = [
        str(WHISPER_CLI), "-m", model_path, str(wav_path),
        "-l", "en", "-t", str(n_threads), "--print-progress",
    ]
    if force_cpu:
        cmd.append("-ng")

    result = subprocess.run(cmd, capture_output=True, text=True, timeout=600)

    # Clean up temp wav
    if str(wav_path) != str(audio_file) and wav_path.exists():
        wav_path.unlink()

    if result.returncode != 0:
        print(f"whisper-cli failed: {result.stderr[:500]}", file=sys.stderr)
        sys.exit(1)

    # Parse timing from stderr
    output = result.stderr or result.stdout
    total_time = _parse_timing(output, "total time")
    duration_s = total_time if total_time > 0 else 0

    segments = _parse_segments(result.stdout)
    full_text = " ".join(s["text"] for s in segments).strip()
    if not segments and result.stdout.strip():
        full_text = result.stdout.strip()

    metadata_dir = session_path / "metadata"
    metadata_dir.mkdir(parents=True, exist_ok=True)
    output_file = metadata_dir / "asr.json"

    payload = {
        "audio_info": {"duration": int(round(duration_s * 1000))},
        "result": {
            "text": full_text,
            "segments": segments,
        },
        "_device": "cpu" if force_cpu else "gpu",
    }
    output_file.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"ASR_OUTPUT:{output_file}")


if __name__ == "__main__":
    main()
