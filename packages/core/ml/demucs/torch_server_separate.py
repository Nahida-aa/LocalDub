"""
Torch server handler for Demucs separate stage.
Imported by pytorch_server.py.

Uses a module-level singleton for the Separator model so it is loaded once
and reused across requests, avoiding repeated GPU memory allocation and
fragmentation.
"""
from __future__ import annotations

import gc
import sys
import time
from pathlib import Path
from typing import Callable

# Reuse _engine.py helpers
REPO_ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO_ROOT / "packages" / "cli" / "src" / "ml" / "demucs"))
from _engine import _demucs_source_path, _demucs_progress  # noqa: PLC0414,E402

_SEPARATOR = None
_SEPARATOR_DEVICE: str = ""


def _detect_vram_gb() -> int | None:
    """Return GPU VRAM in GiB, or None if not detectable."""
    try:
        import torch
        if not torch.cuda.is_available():
            return None
        props = torch.cuda.get_device_properties(0)
        return props.total_mem // (1024 ** 3)
    except Exception:
        return None


def _choose_shifts(device: str) -> int:
    """Use shifts=1 on low-VRAM (≤6 GiB) GPUs to avoid CUDA OOM/fragmentation."""
    if device != "cuda":
        return 3
    vram = _detect_vram_gb()
    if vram is not None and vram <= 6:
        return 1
    return 3


def _get_separator(
    device: str,
    *,
    shifts: int = 3,
    callback: Callable | None = None,
):
    """Get or create the Demucs Separator singleton.

    The model is loaded once and reused across requests to avoid
    repeated GPU memory allocation/fragmentation.
    """
    global _SEPARATOR, _SEPARATOR_DEVICE

    if _SEPARATOR is not None:
        if _SEPARATOR_DEVICE != device:
            # Device changed — release old model and load new one
            del _SEPARATOR
            _SEPARATOR = None
            _SEPARATOR_DEVICE = ""
            gc.collect()
            if device == "cuda":
                import torch
                torch.cuda.empty_cache()
        else:
            # Reuse existing model, just update runtime parameters
            _SEPARATOR.update_parameter(shifts=shifts, callback=callback)
            return _SEPARATOR

    # Clear stale CUDA cache before first load
    if device == "cuda":
        import torch
        torch.cuda.empty_cache()

    demucs_path = _demucs_source_path()
    sys.path.insert(0, str(demucs_path))
    from demucs.api import Separator

    _SEPARATOR = Separator(
        model="htdemucs_ft",
        device=device,
        progress=True,
        shifts=shifts,
        callback=callback,
    )
    _SEPARATOR_DEVICE = device
    return _SEPARATOR


def _release_separator() -> None:
    """Release the singleton Separator to free GPU memory (called on error)."""
    global _SEPARATOR, _SEPARATOR_DEVICE
    if _SEPARATOR is not None:
        del _SEPARATOR
        _SEPARATOR = None
        _SEPARATOR_DEVICE = ""
        gc.collect()
        import torch
        torch.cuda.empty_cache()


def handle_separate(
    params: dict,
    task_id: str,
    *,
    emit: Callable | None = None,
) -> dict:
    """Handle separate stage in torch server mode."""
    from pydub import AudioSegment

    video_path = params["video_path"]
    task_dir = params["task_dir"]
    device = params.get("device", "cpu")

    sep_dir = Path(task_dir) / "separate"
    sep_dir.mkdir(parents=True, exist_ok=True)
    stem_paths = {
        "drums": sep_dir / "target_0_drums.wav",
        "bass": sep_dir / "target_1_bass.wav",
        "other": sep_dir / "target_2_other.wav",
        "vocals": sep_dir / "target_3_vocals.wav",
    }

    shifts = _choose_shifts(device)

    def report_progress(info: dict) -> None:
        progress = _demucs_progress(info, shifts)
        if emit:
            emit({
                "type": "progress",
                "stage": "separate",
                "task_id": task_id,
                "current": progress,
                "total": 100,
            })

    t0 = time.perf_counter()
    separator = _get_separator(device, shifts=shifts, callback=report_progress)
    load_time = time.perf_counter() - t0

    separated = None
    try:
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
    except Exception:
        # On error, release the singleton so next request gets a fresh model
        _release_separator()
        raise
    finally:
        if separated is not None:
            del separated
