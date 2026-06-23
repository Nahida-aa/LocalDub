"""
Daemon handler for Whisper ASR stage.
Imported by pipeline_daemon.py.
"""
from __future__ import annotations

import json
import os
import sys
import time
from pathlib import Path

# Reuse _convert_segments from pytorch.py
REPO_ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(REPO_ROOT / "packages" / "cli" / "src" / "ml" / "whisper"))
from pytorch import _convert_segments  # noqa: PLC0414,E402

_WHISPER = None


def _load_whisper(device: str) -> None:
    global _WHISPER
    if _WHISPER is not None:
        return
    if device == "cuda":
        import torch

        if not torch.cuda.is_available():
            sys.stderr.write("[WARN] torch.cuda.is_available()=False, ASR falling back to CPU\n")
            device = "cpu"
    import whisper

    _WHISPER = whisper.load_model(
        os.getenv("WHISPER_MODEL", "large-v3-turbo"),
        device=device,
        download_root=os.getenv("WHISPER_DOWNLOAD_ROOT") or None,
    )


def handle_asr(params: dict) -> dict:
    from pydub import AudioSegment

    vocals_path = params["vocals_path"]
    session_path = params["session_path"]
    raw_language = params.get("language", "auto")
    language = None if raw_language == "auto" else raw_language
    device = params.get("device", "cpu")

    global _WHISPER
    try:
        _load_whisper(device)
    except Exception as exc:
        if device == "cpu":
            raise
        sys.stderr.write(f"[WARN] GPU whisper load failed ({exc}), falling back to CPU\n")
        _WHISPER = None
        device = "cpu"
        _load_whisper(device)

    t0 = time.perf_counter()
    load_time = time.perf_counter() - t0

    t1 = time.perf_counter()
    result = _WHISPER.transcribe(vocals_path, language=language, word_timestamps=False, verbose=False)
    process_time = time.perf_counter() - t1

    segments = _convert_segments(result.get("segments", []))
    if not segments:
        raise RuntimeError("Whisper did not return any segments.")

    duration_ms = len(AudioSegment.from_file(vocals_path))
    audio_duration_s = duration_ms / 1000.0
    full_text = " ".join(s["text"] for s in segments).strip()
    payload = {
        "audio_info": {"duration": duration_ms},
        "result": {"text": full_text, "segments": segments},
        "_device": device,
    }

    asr_dir = Path(session_path) / "asr"
    asr_dir.mkdir(parents=True, exist_ok=True)
    output_file = asr_dir / "asr.json"
    output_file.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")

    detected = result.get("language", "")
    return {
        "asr_file": str(output_file),
        "detected_language": detected,
        "load_time_s": round(load_time, 3),
        "process_time_s": round(process_time, 3),
        "audio_duration_s": round(audio_duration_s, 3),
        "rtf": round(process_time / audio_duration_s, 3) if audio_duration_s > 0 else 0,
        "actual_device": device,
    }
