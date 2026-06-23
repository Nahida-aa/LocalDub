"""
Pipeline daemon — keeps ML models (Whisper, VoxCPM, Demucs) loaded across video tasks.

Protocol (JSON lines on stdin/stdout or TCP):

  TS ──→ daemon:  {"action":"run_stage","stage":"asr","task_id":"...","params":{...}}
  daemon ──→ TS: {"type":"progress","stage":"asr","current":1,"total":10}
                  {"type":"complete","stage":"asr","output":{...}}
                  {"type":"error","stage":"asr","message":"..."}

Commands:
  run_stage  — execute a pipeline stage (asr | tts | separate)
  shutdown   — graceful exit

On startup, daemon sends {"type":"ready"} then enters stdin read loop.
With --port, also listens on TCP for outliving the spawning process.
Models are lazy-loaded on first use and cached as module-level singletons.

Usage (stdin, spawned by TS):
  PYTHONPATH=submodule/VoxCPM/src:$PYTHONPATH \
  .venv/bin/python packages/cli/scripts/pipeline_daemon.py

Usage (TCP, detached):
  .venv/bin/python packages/cli/scripts/pipeline_daemon.py --port 19109
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import threading
import time
import traceback

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
import wave
from pathlib import Path

# Windows: ensure stdin/stdout use binary mode so JSON lines protocol
# (newline-delimited) is not corrupted by \n → \r\n translation.
if sys.platform == "win32":
    import msvcrt  # noqa: PLC0415
    msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)   # type: ignore[attr-defined]
    msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)  # type: ignore[attr-defined]

# ---------------------------------------------------------------------------
# Model singletons
# ---------------------------------------------------------------------------

_VOXCPM: "VoxCPM | None" = None

# Import Demucs separate handler
REPO_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO_ROOT / "packages" / "cli" / "src" / "ml" / "demucs"))
from daemon_separate import handle_separate  # noqa: PLC0414,E402

# Import Whisper ASR handler
sys.path.insert(0, str(REPO_ROOT / "packages" / "cli" / "src" / "ml" / "whisper"))
from daemon_asr import handle_asr  # noqa: PLC0414,E402


def _load_voxcpm(model_dir: str, device: str) -> None:
	global _VOXCPM
	if _VOXCPM is not None:
		return

	if not os.path.isdir(model_dir) or not os.listdir(model_dir):
		sys.stderr.write(f"[Daemon] Model not found at {model_dir}, attempting download...\n")
		sys.stderr.flush()
		try:
			from modelscope import snapshot_download
			os.makedirs(model_dir, exist_ok=True)
			snapshot_download("OpenBMB/VoxCPM2", local_dir=model_dir)
			sys.stderr.write("[Daemon] Model downloaded via ModelScope\n")
		except Exception as exc:
			sys.stderr.write(f"[Daemon] ModelScope download failed ({exc}), VoxCPM.from_pretrained will try HuggingFace\n")
		sys.stderr.flush()

	from voxcpm import VoxCPM

	_VOXCPM = VoxCPM.from_pretrained(model_dir, load_denoiser=False, device=device)


# ---------------------------------------------------------------------------
# Stage handlers
# ---------------------------------------------------------------------------

def _write_wav(wav, path: str, sample_rate: int = 48000) -> None:
    import numpy as np

    wav_int16 = np.clip(wav * 32767, -32768, 32767).astype(np.int16)
    with wave.open(path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(sample_rate)
        wf.writeframes(wav_int16.tobytes())


def _write_empty_wav(path: str) -> None:
    with wave.open(path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(48000)
        wf.writeframes(b"")


# ---------------------------------------------------------------------------
# TTS (VoxCPM)
# ---------------------------------------------------------------------------

def handle_tts(params: dict, task_id: str, *, emit=None) -> dict:
    translation_file = Path(params["translation_file"])
    vocals_dir = Path(params["vocals_dir"])
    tts_dir = Path(params["tts_dir"])
    model_dir = params["model_dir"]
    device = params.get("device", "cpu")
    cfg_value = float(params.get("cfg_value", 2.0))
    timesteps = int(params.get("inference_timesteps", 10))
    skip_existing = bool(params.get("skipExisting", False))

    tts_dir.mkdir(parents=True, exist_ok=True)

    t0 = time.perf_counter()
    _load_voxcpm(model_dir, device)
    load_time = time.perf_counter() - t0

    data = json.loads(translation_file.read_text(encoding="utf-8"))
    items = data["translation"]
    total = len(items)
    if total == 0:
        return {"generated": 0, "skipped": 0, "errors": 0, "generate_time_s": 0, "load_time_s": round(load_time, 3)}

    min_bytes = 1200 * 16 * 2
    fallback = ""
    for f in sorted(vocals_dir.glob("*.wav")):
        if f.stat().st_size >= min_bytes:
            fallback = str(f)
            break

    generated = skipped = errors = 0
    gen_time = 0.0

    for index, item in enumerate(items, start=1):
        idx = f"{index:04d}"
        out_path = tts_dir / f"{idx}.wav"

        ref_path = vocals_dir / f"{idx}.wav"
        if not ref_path.exists() or ref_path.stat().st_size < min_bytes:
            ref_path = Path(fallback) if fallback else None
        ref_mtime = ref_path.stat().st_mtime_ns if ref_path and ref_path.exists() else 0

        if skip_existing and out_path.exists() and out_path.stat().st_mtime_ns > ref_mtime:
            skipped += 1
            _emit_progress("tts", task_id, index, total, emit=emit)
            continue

        text = item.get("dst") or item.get("zh", "")
        if not text.strip():
            _write_empty_wav(str(out_path))
            skipped += 1
            _emit_progress("tts", task_id, index, total, emit=emit)
            continue

        if ref_path is None or not ref_path.exists():
            _write_empty_wav(str(out_path))
            skipped += 1
            _emit_progress("tts", task_id, index, total, emit=emit)
            continue

        t1 = time.perf_counter()
        try:
            wav = _VOXCPM.generate(
                text=text,
                reference_wav_path=str(ref_path),
                cfg_value=cfg_value,
                inference_timesteps=timesteps,
            )
            _write_wav(wav, str(out_path), 48000)
            gen_time += time.perf_counter() - t1
            generated += 1
        except Exception as e:
            errors += 1
            sys.stderr.write(f"[ERROR] Segment {idx} failed: {e}\n")
            sys.stderr.flush()
            _write_empty_wav(str(out_path))

        _emit_progress("tts", task_id, index, total, emit=emit)

    total_time = time.perf_counter() - t0
    out_dur = sum(
        item.get("end_time", 0) - item.get("start_time", 0)
        for item in items
    ) / 1000.0
    rtf = round(gen_time / max(out_dur, 0.001), 3) if generated > 0 and out_dur > 0 else 0
    return {
        "generated": generated,
        "skipped": skipped,
        "errors": errors,
        "generate_time_s": round(gen_time, 3),
        "load_time_s": round(load_time, 3),
        "total_time_s": round(total_time, 3),
        "rtf": rtf,
    }


# ---------------------------------------------------------------------------
# Separate (Demucs)
# ---------------------------------------------------------------------------

# ---------------------------------------------------------------------------
# IO helpers — all accept optional emit override for TCP output
# ---------------------------------------------------------------------------

def _emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def _emit_progress(stage: str, task_id: str, current: int, total: int, *, emit=None) -> None:
    emitter = emit or _emit
    emitter({"type": "progress", "stage": stage, "task_id": task_id, "current": current, "total": total})


def _emit_complete(stage: str, task_id: str, output: dict, *, emit=None) -> None:
    emitter = emit or _emit
    emitter({"type": "complete", "stage": stage, "task_id": task_id, "output": output})


def _emit_error(stage: str, task_id: str, message: str, *, emit=None) -> None:
    emitter = emit or _emit
    emitter({"type": "error", "stage": stage, "task_id": task_id, "message": message})


# ---------------------------------------------------------------------------
# Command dispatch
# ---------------------------------------------------------------------------

def process_command(cmd: dict, *, emit=None) -> bool:
    """Process one command. Returns False if shutdown was requested."""
    action = cmd.get("action", "")
    if action == "shutdown":
        return False

    if action == "run_stage":
        stage = cmd.get("stage", "")
        task_id = cmd.get("task_id", "")
        params = cmd.get("params", {})
        try:
            if stage == "asr":
                output = handle_asr(params)
            elif stage == "tts":
                output = handle_tts(params, task_id, emit=emit)
            elif stage == "separate":
                output = handle_separate(params, task_id, emit=emit)
            else:
                _emit_error(stage, task_id, f"Unknown stage: {stage}", emit=emit)
                return True
            _emit_complete(stage, task_id, output, emit=emit)
        except Exception as e:
            tb = traceback.format_exc()
            sys.stderr.write(f"[{stage}] {tb}\n")
            sys.stderr.flush()
            _emit_error(stage, task_id, str(e), emit=emit)
    else:
        sys.stderr.write(f"Unknown action: {action}\n")
        sys.stderr.flush()

    return True


# ---------------------------------------------------------------------------
# TCP server (optional, for persistent mode)
# ---------------------------------------------------------------------------

_IDLE_TIMEOUT = 1800  # seconds — exit if no connection for this long

_shutdown = False


def tcp_server(port: int) -> None:
    import socket as _socket

    global _shutdown

    sock = _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM)
    sock.setsockopt(_socket.SOL_SOCKET, _socket.SO_REUSEADDR, 1)
    sock.bind(("127.0.0.1", port))
    sock.listen(5)
    sock.settimeout(_IDLE_TIMEOUT)
    sys.stderr.write(f"[Daemon] TCP listening on 127.0.0.1:{port} (idle timeout={_IDLE_TIMEOUT}s)\n")
    sys.stderr.flush()

    while not _shutdown:
        try:
            conn, addr = sock.accept()
        except _socket.timeout:
            sys.stderr.write("[Daemon] Idle timeout, shutting down\n")
            sys.stderr.flush()
            _shutdown = True
            break
        except Exception as e:
            if not _shutdown:
                sys.stderr.write(f"[Daemon] Accept error: {e}\n")
                sys.stderr.flush()
            break

        with conn:
            f = conn.makefile("rw", encoding="utf-8")
            def _tcp_emit(obj: dict) -> None:
                f.write(json.dumps(obj, ensure_ascii=False) + "\n")
                f.flush()

            for raw in f:
                line = raw.strip()
                if not line:
                    continue
                try:
                    cmd = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if not process_command(cmd, emit=_tcp_emit):
                    _tcp_emit({"type": "shutdown"})
                    _shutdown = True
                    break


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="ML Pipeline Daemon")
    parser.add_argument("--port", type=int, default=0, help="TCP port (0 = stdin-only)")
    args = parser.parse_args()

    tcp_thread: threading.Thread | None = None
    if args.port:
        tcp_thread = threading.Thread(target=tcp_server, args=(args.port,), daemon=True)
        tcp_thread.start()

    _emit({"type": "ready"})

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        if _shutdown:
            break
        try:
            cmd = json.loads(line)
        except json.JSONDecodeError:
            sys.stderr.write(f"Invalid JSON from stdin: {line}\n")
            sys.stderr.flush()
            continue
        if not process_command(cmd):
            break

    _emit({"type": "shutdown"})

    if tcp_thread:
        # In TCP-only mode (detached, stdin EOF), main thread must stay alive
        # to keep the daemon thread running. Wait for shutdown signal.
        tcp_thread.join()


if __name__ == "__main__":
    main()
