"""
Torch server — FastAPI + uvicorn HTTP server.

Endpoints:
  GET  /                           → status dashboard (Solid.js + TailwindCSS)
  GET  /api/events                 → SSE stream: health (3s) + log (on change)
  GET  /api/health                 → server status and model states
  GET  /api/logs                   → server log lines (text/plain lines)
  POST /api/run/{stage}            → execute a stage, returns SSE stream
  POST /api/shutdown               → graceful shutdown

SSE events from /api/run/{stage}:
  event: progress
  data: {"current":50,"total":100}

  event: complete
  data: {"output":{...}}

  event: error
  data: {"message":"..."}

Usage (detached, spawned by TS):
  .venv/bin/python packages/demucs_torch_server/pytorch_server.py --http-port 19109

Check health:
  curl http://127.0.0.1:19109/api/health
"""

from __future__ import annotations

import argparse
import asyncio
import html
import json
import os
import sys
import time
import traceback
from collections import deque
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

# Reduce CUDA memory fragmentation — recommended by PyTorch docs for
# workloads that repeatedly allocate/free large tensors (demucs + ASR).
os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")

# On Windows, explicitly register torch/torchaudio DLL directories before
# any torch import. os.add_dll_directory() is more reliable than PATH
# manipulation, since conda base env DLLs can still leak through.
if sys.platform == "win32":
    _venv = Path(__file__).resolve().parents[2] / ".venv" / "Lib" / "site-packages"
    for _d in ("torch", "torchaudio"):
        _lib = _venv / _d / "lib"
        if _lib.is_dir():
            os.add_dll_directory(str(_lib))

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, HTMLResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles

# ---------------------------------------------------------------------------
# Server log buffer
# ---------------------------------------------------------------------------

class _LogBuffer:
    def __init__(self, max_lines: int = 500):
        self._lines: deque[str] = deque(maxlen=max_lines)
        self._write_count: int = 0

    def write(self, text: str) -> None:
        for line in text.rstrip("\n").split("\n"):
            if line:
                self._lines.append(line)
                self._write_count += 1

    def flush(self) -> None:
        pass

    def get_lines(self, n: int = 100) -> list[str]:
        return list(self._lines)[-n:]

    def get_text(self, n: int = 100) -> str:
        return "\n".join(self.get_lines(n))

    @property
    def write_count(self) -> int:
        return self._write_count

_log_buffer = _LogBuffer()
_log_writer = _LogBuffer()

# Wrap stdout/stderr so all server-side output is captured
class _Tee:
    def __init__(self, stream, buffer: _LogBuffer):
        self._stream = stream
        self._buffer = buffer

    def write(self, text: str) -> None:
        self._stream.write(text)
        self._buffer.write(text)

    def flush(self) -> None:
        self._stream.flush()

    def reconfigure(self, **kwargs):
        if hasattr(self._stream, 'reconfigure'):
            self._stream.reconfigure(**kwargs)

    def isatty(self):
        return False

    @property
    def encoding(self):
        return getattr(self._stream, 'encoding', 'utf-8')

sys.stdout = _Tee(sys.stdout, _log_buffer)
sys.stderr = _Tee(sys.stderr, _log_buffer)

# ---------------------------------------------------------------------------
# Model handler imports
# ---------------------------------------------------------------------------
REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "packages" / "core" / "ml" / "demucs"))
sys.path.insert(0, str(REPO_ROOT / "packages" / "core" / "ml" / "whisper"))

# mDNS service discovery
from servers_py.mdns_server import register_service, unregister_service  # noqa: E402

from torch_server_separate import handle_separate  # noqa: PLC0414,E402
from torch_server_asr import handle_asr  # noqa: PLC0414,E402

# ---------------------------------------------------------------------------
# FastAPI app
# ---------------------------------------------------------------------------

app = FastAPI(title="ML Torch Server")
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)
_shutdown = False
_start_time = time.time()
_executor = ThreadPoolExecutor(max_workers=1)
_SERVER_PORT: int = 19109

# Track which models have been loaded
_models: dict[str, dict[str, str]] = {
    "asr": {"status": "unloaded", "device": ""},
    "separate": {"status": "unloaded", "device": ""},
}


# ---------------------------------------------------------------------------
# SSE helpers
# ---------------------------------------------------------------------------

def _emit_to_queue(queue: asyncio.Queue, obj: dict) -> None:
    """Wrap an emit-style dict into an SSE event and push to the queue."""
    event_type = obj.get("type", "message")
    data = {k: v for k, v in obj.items() if k != "type"}
    queue.put_nowait({"event": event_type, "data": data})


async def _run_stage_events(stage: str, task_id: str, params: dict):
    """Async generator yielding SSE events for a stage run."""
    queue: asyncio.Queue = asyncio.Queue()
    last_log_count = _log_buffer.write_count

    def emit(obj: dict) -> None:
        _emit_to_queue(queue, obj)

    def emit_log_line(line: str) -> None:
        """Forward a log line back into the queue so it's yielded in-band."""
        emit({"type": "log", "line": line})

    def worker() -> None:
        try:
            dev = params.get("device", "cpu")
            if stage == "asr":
                _models["asr"] = {"status": "loading", "device": dev}
                output = handle_asr(params)
                _models["asr"] = {"status": "ready", "device": dev}
                emit({"type": "complete", "output": output})
            elif stage == "separate":
                _models["separate"] = {"status": "loading", "device": dev}
                output = handle_separate(params, task_id, emit=emit)
                _models["separate"] = {"status": "ready", "device": dev}
                emit({"type": "complete", "output": output})
            else:
                emit({"type": "error", "message": f"Unknown stage: {stage}"})
        except Exception:
            traceback.print_exc(file=sys.stderr)
            sys.stderr.flush()
            if stage in _models:
                _models[stage] = {"status": "error", "device": _models[stage].get("device", "")}
            emit({"type": "error", "message": traceback.format_exc()})

    loop = asyncio.get_event_loop()
    loop.run_in_executor(_executor, worker)

    while True:
        # Real-time: no timeout, yields immediately when the queue has data
        event = await queue.get()

        # Before yielding, flush any accumulated log lines into the queue
        current_count = _log_buffer.write_count
        if current_count > last_log_count:
            for line in _log_buffer.get_lines(current_count - last_log_count):
                if line.startswith('\r') and '%|' in line and '| ' in line:
                    continue
                if not line.strip():
                    continue
                emit_log_line(line)
            last_log_count = current_count

        yield f"event: {event['event']}\ndata: {json.dumps(event['data'], ensure_ascii=False)}\n\n"
        if event["event"] in ("complete", "error"):
            break


# ---------------------------------------------------------------------------
# API endpoints (define BEFORE static files to avoid mount conflict)
# ---------------------------------------------------------------------------


@app.get("/api/logs")
async def get_logs(n: int = 100):
    return {"lines": "\n".join(_log_buffer.get_lines(n))}


@app.get("/status")
async def health():
    return {
        "status": "running",
        "port": _SERVER_PORT,
        "uptime_s": int(time.time() - _start_time),
        "models": {name: {"status": status} for name, status in _models.items()},
    }


@app.post("/api/run/{stage}")
async def run_stage(stage: str, body: dict):
    task_id = body.get("task_id", "")
    params = body.get("params", {})
    return StreamingResponse(
        _run_stage_events(stage, task_id, params),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )


@app.get("/api/events")
async def events():
    """SSE endpoint — pushes health every 3s and log lines on change."""

    async def event_generator():
        last_count = _log_buffer.write_count
        while not _shutdown:
            # health event every tick
            health_data = {
                "status": "running",
                "port": _SERVER_PORT,
        "models": {name: {"status": info["status"], "device": info.get("device", "")} for name, info in _models.items()},
            }
            yield f"event: health\ndata: {json.dumps(health_data)}\n\n"

            # log event only when new lines arrived
            current_count = _log_buffer.write_count
            if current_count != last_count:
                new_lines = "\n".join(_log_buffer.get_lines(100))
                yield f"event: log\ndata: {json.dumps({'lines': new_lines, 'count': current_count})}\n\n"
                last_count = current_count

            await asyncio.sleep(3)

    return StreamingResponse(
        event_generator(),
        media_type="text/event-stream",
        headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"},
    )


@app.post("/api/shutdown")
async def shutdown():
    global _shutdown
    _shutdown = True
    # Schedule server shutdown in next event loop iteration
    asyncio.get_event_loop().call_later(0.1, lambda: __import__("os")._exit(0))
    return {"status": "shutting_down"}


# ---------------------------------------------------------------------------
# Static dashboard (Solid.js built output) — mounted last so API routes win
# ---------------------------------------------------------------------------

_DASHBOARD_DIR = REPO_ROOT / "packages" / "web" / "dist"
if _DASHBOARD_DIR.is_dir():
    app.mount("/", StaticFiles(directory=str(_DASHBOARD_DIR), html=True), name="dashboard")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="ML Torch Server (HTTP)")
    parser.add_argument(
        "--http-port", type=int, default=19109, help="HTTP port (default: 19109)"
    )
    parser.add_argument(
        "--host", type=str, default="127.0.0.1", help="Bind address (default: 127.0.0.1)"
    )
    parser.add_argument("--reload", action="store_true", help="Enable hot reload for development")
    args = parser.parse_args()
    global _SERVER_PORT
    _SERVER_PORT = args.http_port

    # Register mDNS so CLI/UI can discover the actual port
    _mdns_zc = register_service("demucs_torch_server", args.http_port)
    print(f"PORT={args.http_port}", flush=True)
    print(
        f"[TorchServer] Starting HTTP server on {args.host}:{args.http_port}",
        flush=True,
    )
    try:
        if args.reload:
            import sys
            sys.modules[__name__]._uvicorn_app = app
            uvicorn.run("pytorch_server:_uvicorn_app", host=args.host, port=args.http_port, log_level="info", reload=True)
        else:
            uvicorn.run(app, host=args.host, port=args.http_port, log_level="info")
    finally:
        unregister_service(_mdns_zc)


if __name__ == "__main__":
    main()
