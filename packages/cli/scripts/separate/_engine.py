from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Callable

REPO_ROOT = Path(__file__).resolve().parents[4]


def _device() -> str:
    configured = os.getenv("DEMUCS_DEVICE", "").strip()
    if not configured:
        configured = os.getenv("DEVICE", "").strip()
    if not configured:
        return "cuda"
    device = configured.lower()
    if device in ("gpu", "auto"):
        return "cuda"
    if device in ("cpu", "cuda", "mps"):
        return device
    return "cuda"



def _demucs_progress(info: dict, shifts: int) -> int:
    models = max(1, int(info.get("models") or 1))
    model_index = max(0, int(info.get("model_idx_in_bag") or 0))
    shift_index = max(0, int(info.get("shift_idx") or 0))
    audio_length = max(0, int(info.get("audio_length") or 0))
    segment_offset = max(0, int(info.get("segment_offset") or 0))
    segment_ratio = min(segment_offset / audio_length, 1) if audio_length else 0
    total_units = max(1, models * shifts)
    completed_units = model_index * shifts + shift_index + segment_ratio
    return max(0, min(99, int(completed_units / total_units * 100)))


def _demucs_source_path() -> Path:
    demucs_path = REPO_ROOT / "submodule" / "demucs"
    api_file = demucs_path / "demucs" / "api.py"
    if api_file.exists():
        return demucs_path
    raise RuntimeError(
        "Demucs source submodule is missing or incomplete. "
        "Clone this repository with git and run: git submodule update --init --recursive. "
        "Do not use GitHub Download ZIP because it does not include submodules."
    )


def load_separator(shifts: int = 3) -> Path:
    """Import demucs, construct Separator with full progress, return demucs_path.
    Used by --benchmark-load to measure model load time independently."""
    demucs_path = _demucs_source_path()
    sys.path.insert(0, str(demucs_path))
    from demucs.api import Separator  # noqa: PLC0415
    Separator(
        model="htdemucs_ft",
        device=_device(),
        progress=True,
        shifts=shifts,
    )
    return demucs_path


def separate_audio(
    video_file: Path | str,
    session: Path | str,
    progress_callback: Callable[[int, str], None] | None = None,
    shifts: int = 3,
) -> tuple[Path, Path]:
    video_file = Path(video_file)
    session = Path(session)

    demucs_path = _demucs_source_path()
    sys.path.insert(0, str(demucs_path))

    from demucs.api import Separator, save_audio

    media_dir = session / "media"
    media_dir.mkdir(parents=True, exist_ok=True)
    vocals_file = media_dir / "audio_vocals.wav"
    bgm_file = media_dir / "audio_bgm.wav"
    if vocals_file.exists() and bgm_file.exists():
        return vocals_file, bgm_file

    def report_progress(info: dict) -> None:
        if progress_callback is None:
            return
        progress = _demucs_progress(info, shifts)
        progress_callback(progress, f"Separating audio {progress}%")

    separator = Separator(
        model="htdemucs_ft",
        device=_device(),
        progress=True,
        shifts=shifts,
        callback=report_progress,
    )
    _, separated = separator.separate_audio_file(str(video_file))

    vocals = separated["vocals"]
    bgm = None
    for stem, source in separated.items():
        if stem == "vocals":
            continue
        bgm = source if bgm is None else bgm + source

    save_audio(vocals, str(vocals_file), samplerate=separator.samplerate)
    save_audio(bgm, str(bgm_file), samplerate=separator.samplerate)
    return vocals_file, bgm_file
