"""
CLI ASR using faster-whisper (CTranslate2) with GPU (float16) → CPU (float32) fallback.

Usage:
    .venv/bin/python packages/cli/scripts/asr/run.py <vocals_wav> <session_path> [language] [--cpu] [--compute-type float16|float32|int8]
    .venv/bin/python packages/cli/scripts/asr/run.py --benchmark-load [--cpu] [--compute-type float16|float32|int8]

Defaults: GPU → float16, CPU → float32 (aligned with PyTorch).
Output:
    Writes asr.json to <session_path>/metadata/asr.json
    Prints the output path on success, exits 0.
    On failure, prints error to stderr, exits 1.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

HSA_OVERRIDE = "HSA_OVERRIDE_GFX_VERSION"
EXPECTED_HSA = "11.0.0"


def _parse_compute_type() -> str | None:
    if "--compute-type" in sys.argv[1:]:
        idx = sys.argv[1:].index("--compute-type") + 1
        if idx < len(sys.argv):
            return sys.argv[idx + 1]
    return None


def _set_hsa_override() -> None:
    val = os.environ.get(HSA_OVERRIDE)
    if val != EXPECTED_HSA:
        os.environ[HSA_OVERRIDE] = EXPECTED_HSA


def _wake_gpu() -> None:
    """Force GPU init to ensure clean state after previous hangs."""
    import torch

    _ = torch.cuda.device_count()
    _ = torch.cuda.mem_get_info()


def _transcribe_gpu(model_path: str, audio_path: str, language: str, compute_type: str = "float16") -> tuple:
    _set_hsa_override()
    _wake_gpu()
    from faster_whisper import WhisperModel

    model = WhisperModel(model_path, device="cuda", compute_type=compute_type)
    segments, info = model.transcribe(audio_path, language=language)
    return list(segments), info


def _transcribe_cpu(model_path: str, audio_path: str, language: str, compute_type: str = "float32") -> tuple:
    from faster_whisper import WhisperModel

    model = WhisperModel(model_path, device="cpu", compute_type=compute_type)
    segments, info = model.transcribe(audio_path, language=language)
    return list(segments), info


def _convert(segments: list, info) -> dict:
    segs = []
    for seg in segments:
        segs.append(
            {
                "text": (seg.text or "").strip(),
                "start": seg.start,
                "end": seg.end,
                "words": [],
            }
        )
    full_text = " ".join(s["text"] for s in segs).strip()
    return {
        "audio_info": {"duration": int(round(info.duration * 1000))},
        "result": {
            "text": full_text,
            "segments": segs,
        },
    }


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    force_cpu = "--cpu" in sys.argv[1:]
    compute_type = _parse_compute_type()

    if "--benchmark-load" in sys.argv[1:]:
        model_name = os.environ.get("WHISPER_MODEL", "large-v3-turbo")
        from faster_whisper import WhisperModel
        if force_cpu:
            ct = compute_type or "float32"
            _ = WhisperModel(model_name, device="cpu", compute_type=ct)
        else:
            ct = compute_type or "float16"
            _ = WhisperModel(model_name, device="cuda", compute_type=ct)
        print("[BENCHMARK_LOAD_DONE]")
        return

    if len(args) < 2:
        print(f"Usage: {sys.argv[0]} <vocals_wav> <session_path> [language] [--cpu]", file=sys.stderr)
        sys.exit(1)

    vocals_file = Path(args[0])
    session_path = Path(args[1])
    language = None if args[2] == "auto" else args[2] if len(args) > 2 else None
    model_name = os.environ.get("WHISPER_MODEL", "large-v3-turbo")

    if not vocals_file.is_file():
        print(f"Error: vocals file not found: {vocals_file}", file=sys.stderr)
        sys.exit(1)

    metadata_dir = session_path / "metadata"
    metadata_dir.mkdir(parents=True, exist_ok=True)
    output_file = metadata_dir / "asr.json"

    device_used = "cpu" if force_cpu else "gpu"

    if force_cpu:
        ct = compute_type or "float32"
        segments, info = _transcribe_cpu(model_name, str(vocals_file), language, ct)
    else:
        ct = compute_type or "float16"
        try:
            segments, info = _transcribe_gpu(model_name, str(vocals_file), language, ct)
        except Exception as exc:
            sys.stderr.write(f"GPU transcribe failed: {exc}\n")
            sys.stderr.write("Falling back to CPU (float32)...\n")
            try:
                segments, info = _transcribe_cpu(model_name, str(vocals_file), language, "float32")
                device_used = "cpu"
            except Exception as exc2:
                print(f"CPU transcribe also failed: {exc2}", file=sys.stderr)
                sys.exit(1)

    data = _convert(segments, info)
    data["_device"] = device_used
    data["_compute_type"] = ct
    data["detected_language"] = info.language

    output_file.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"ASR_OUTPUT:{output_file}")


if __name__ == "__main__":
    main()
