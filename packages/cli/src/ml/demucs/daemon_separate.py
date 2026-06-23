"""
Daemon handler for Demucs separate stage.
Imported by pipeline_daemon.py.
"""
from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Callable

# Reuse _engine.py helpers
REPO_ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(REPO_ROOT / "packages" / "cli" / "src" / "ml" / "demucs"))
from _engine import _demucs_source_path, _demucs_progress  # noqa: PLC0414,E402


def _load_demucs(device: str):
    """Load and return the Demucs Separator class."""
    demucs_path = _demucs_source_path()
    sys.path.insert(0, str(demucs_path))
    from demucs.api import Separator

    return Separator


def handle_separate(
    params: dict,
    task_id: str,
    *,
    emit: Callable | None = None,
) -> dict:
    """Handle separate stage in daemon mode."""
    from pydub import AudioSegment

    video_path = params["video_path"]
    session_path = params["session_path"]
    device = params.get("device", "cpu")

    t0 = time.perf_counter()
    Separator = _load_demucs(device)

    sep_dir = Path(session_path) / "separate"
    sep_dir.mkdir(parents=True, exist_ok=True)
    stem_paths = {
        "drums": sep_dir / "target_0_drums.wav",
        "bass": sep_dir / "target_1_bass.wav",
        "other": sep_dir / "target_2_other.wav",
        "vocals": sep_dir / "target_3_vocals.wav",
    }

    shifts = 3

    def report_progress(info: dict) -> None:
        progress = _demucs_progress(info, shifts)
        if emit:
            emit({"type": "progress", "stage": "separate", "task_id": task_id, "current": progress, "total": 100})

    separator = Separator(
        model="htdemucs_ft",
        device=device,
        progress=True,
        shifts=shifts,
        callback=report_progress,
    )
    load_time = time.perf_counter() - t0

    t1 = time.perf_counter()
    _, separated = separator.separate_audio_file(video_path)
    process_time = time.perf_counter() - t1

    audio_duration_s = len(AudioSegment.from_file(video_path)) / 1000.0

    from demucs.api import save_audio

    for stem, path in stem_paths.items():
        save_audio(separated[stem], str(path), samplerate=separator.samplerate)

    return {
        "vocals_file": str(stem_paths["vocals"]),
        "load_time_s": round(load_time, 3),
        "process_time_s": round(process_time, 3),
        "audio_duration_s": round(audio_duration_s, 3),
        "rtf": round(process_time / audio_duration_s, 3) if audio_duration_s > 0 else 0,
    }
