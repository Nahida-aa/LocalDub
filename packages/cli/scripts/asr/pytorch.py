"""
PyTorch ASR using openai-whisper, standalone (no backend/ dependency).

Usage:
    .venv/bin/python packages/cli/scripts/asr/pytorch.py <vocals_wav> <session_path> [language] [--device cpu|cuda]
    .venv/bin/python packages/cli/scripts/asr/pytorch.py --benchmark-load [--device cpu|cuda]

Reads WHISPER_MODEL / WHISPER_DEVICE / DEVICE env vars.
Writes asr.json to <session_path>/metadata/.
"""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import torch
import torch.nn.functional as F

_original_conv1d = F.conv1d
_original_conv2d = F.conv2d
_original_conv_transpose1d = F.conv_transpose1d
_original_conv_transpose2d = F.conv_transpose2d


def _unpack1d(v, name="param"):
    if isinstance(v, int):
        return v
    if isinstance(v, tuple):
        if len(v) == 1:
            return v[0]
        if len(v) == 0:
            return 1
    return v


def _unpack2d(v):
    if isinstance(v, int):
        return (v, v)
    if isinstance(v, tuple):
        if len(v) == 1:
            return (v[0], v[0])
        return (v[0], v[1])
    return v


def _conv1d_gemm(input, weight, bias=None, stride=1, padding=0, dilation=1, groups=1):
    stride = _unpack1d(stride)
    padding = _unpack1d(padding)
    dilation = _unpack1d(dilation)
    N, C_in, L = input.shape
    C_out, C_per_group, K = weight.shape
    if groups == 1 and C_per_group == C_in:
        return _conv1d_gemm_single(input, weight, bias, stride, padding, dilation)
    g_out = C_out // groups
    outputs = []
    for g in range(groups):
        g_in = input[:, g * C_per_group : (g + 1) * C_per_group, :]
        g_w = weight[g * g_out : (g + 1) * g_out, :, :]
        g_b = bias[g * g_out : (g + 1) * g_out] if bias is not None else None
        outputs.append(_conv1d_gemm_single(g_in, g_w, g_b, stride, padding, dilation))
    return torch.cat(outputs, dim=1)


def _conv1d_gemm_single(input, weight, bias, stride, padding, dilation):
    N, C_in, L = input.shape
    C_out, _, K = weight.shape
    x = input.unsqueeze(2)
    patches = F.unfold(x, kernel_size=(1, K), dilation=(1, dilation), padding=(0, padding), stride=(1, stride))
    w_flat = weight.reshape(C_out, -1)
    out = w_flat @ patches
    if bias is not None:
        out += bias.unsqueeze(0).unsqueeze(2)
    return out


def _conv2d_gemm(input, weight, bias=None, stride=1, padding=0, dilation=1, groups=1):
    stride = _unpack2d(stride)
    padding = _unpack2d(padding)
    dilation = _unpack2d(dilation)
    N, C_in, H, W = input.shape
    C_out, C_per_group, KH, KW = weight.shape
    if groups == 1 and C_per_group == C_in:
        return _conv2d_gemm_single(input, weight, bias, stride, padding, dilation)
    g_out = C_out // groups
    outputs = []
    for g in range(groups):
        g_in = input[:, g * C_per_group : (g + 1) * C_per_group, :, :]
        g_w = weight[g * g_out : (g + 1) * g_out, :, :, :]
        g_b = bias[g * g_out : (g + 1) * g_out] if bias is not None else None
        outputs.append(_conv2d_gemm_single(g_in, g_w, g_b, stride, padding, dilation))
    return torch.cat(outputs, dim=1)


def _conv2d_gemm_single(input, weight, bias, stride, padding, dilation):
    N, C_in, H, W = input.shape
    C_out, _, KH, KW = weight.shape
    patches = F.unfold(input, kernel_size=(KH, KW), dilation=dilation, padding=padding, stride=stride)
    w_flat = weight.reshape(C_out, -1)
    out = w_flat @ patches
    H_out = (H + 2 * padding[0] - dilation[0] * (KH - 1) - 1) // stride[0] + 1
    W_out = (W + 2 * padding[1] - dilation[1] * (KW - 1) - 1) // stride[1] + 1
    out = out.reshape(N, C_out, H_out, W_out)
    if bias is not None:
        out += bias.unsqueeze(0).unsqueeze(2).unsqueeze(3)
    return out


def _conv_transpose1d_gemm(input, weight, bias=None, stride=1, padding=0, output_padding=0, groups=1, dilation=1):
    stride = _unpack1d(stride)
    padding = _unpack1d(padding)
    output_padding = _unpack1d(output_padding)
    dilation = _unpack1d(dilation)
    N, C_in, L_in = input.shape
    C_in_w, C_out_per_group, K = weight.shape
    if groups == 1:
        return _conv_transpose1d_gemm_single(input, weight, bias, stride, padding, output_padding, dilation)
    g_in = C_in // groups
    g_out = C_out_per_group
    outputs = []
    for g in range(groups):
        g_input = input[:, g * g_in : (g + 1) * g_in, :]
        g_w = weight[g * g_in : (g + 1) * g_in, :, :]
        g_b = bias[g * g_out : (g + 1) * g_out] if bias is not None else None
        outputs.append(_conv_transpose1d_gemm_single(g_input, g_w, g_b, stride, padding, output_padding, dilation))
    return torch.cat(outputs, dim=1)


def _conv_transpose1d_gemm_single(input, weight, bias, stride, padding, output_padding, dilation):
    N, C_in, L_in = input.shape
    _, C_out, K = weight.shape
    if dilation > 1:
        K_eff = dilation * (K - 1) + 1
        w_d = torch.zeros(C_in, C_out, K_eff, device=input.device, dtype=input.dtype)
        for i in range(K):
            w_d[:, :, i * dilation] = weight[:, :, i]
        weight = w_d
        K = K_eff
        dilation = 1
    if stride > 1:
        L_up = (L_in - 1) * stride + 1
        x_up = torch.zeros(N, C_in, L_up, device=input.device, dtype=input.dtype)
        x_up[:, :, ::stride] = input
        input = x_up
        L_in = L_up
        stride = 1
    L_out = L_in - 2 * padding + K - 1 + output_padding + 1
    w_flipped = weight.flip(-1)
    w_for_conv = w_flipped.permute(1, 0, 2)
    pad_adj = K - 1 - padding
    x_pad = F.pad(input, (pad_adj, pad_adj))
    out = _conv1d_gemm(x_pad, w_for_conv, None, stride=1, padding=0, dilation=1, groups=1)
    out = out[:, :, :L_out]
    if bias is not None:
        out += bias.unsqueeze(0).unsqueeze(2)
    return out


def _conv_transpose2d_gemm(input, weight, bias=None, stride=1, padding=0, output_padding=0, groups=1, dilation=1):
    s_h, s_w = _unpack2d(stride)
    p_h, p_w = _unpack2d(padding)
    op_h, op_w = _unpack2d(output_padding)
    d_h, d_w = _unpack2d(dilation)
    N, C_in, H_in, W_in = input.shape
    _, C_out, KH, KW = weight.shape
    if d_h > 1 or d_w > 1:
        KH_eff = d_h * (KH - 1) + 1
        KW_eff = d_w * (KW - 1) + 1
        w_d = torch.zeros(C_in, C_out, KH_eff, KW_eff, device=input.device, dtype=input.dtype)
        for i in range(KH):
            for j in range(KW):
                w_d[:, :, i * d_h, j * d_w] = weight[:, :, i, j]
        weight = w_d
        KH, KW = KH_eff, KW_eff
        d_h, d_w = 1, 1
    if s_h > 1 or s_w > 1:
        H_up = (H_in - 1) * s_h + 1
        W_up = (W_in - 1) * s_w + 1
        x_up = torch.zeros(N, C_in, H_up, W_up, device=input.device, dtype=input.dtype)
        x_up[:, :, ::s_h, ::s_w] = input
        input = x_up
        H_in, W_in = H_up, W_up
        s_h, s_w = 1, 1
    H_out = H_in - 2 * p_h + KH - 1 + op_h + 1
    W_out = W_in - 2 * p_w + KW - 1 + op_w + 1
    w_flipped = weight.flip(-1).flip(-2)
    w_for_conv = w_flipped.permute(1, 0, 2, 3)
    pad_h = KH - 1 - p_h
    pad_w = KW - 1 - p_w
    x_pad = F.pad(input, (pad_w, pad_w, pad_h, pad_h))
    out = _conv2d_gemm(x_pad, w_for_conv, None, stride=1, padding=0, dilation=1, groups=groups)
    out = out[:, :, :H_out, :W_out]
    if bias is not None:
        out += bias.unsqueeze(0).unsqueeze(2).unsqueeze(3)
    return out


def _apply_conv_patch():
    F.conv1d = _conv1d_gemm
    F.conv2d = _conv2d_gemm
    F.conv_transpose1d = _conv_transpose1d_gemm
    F.conv_transpose2d = _conv_transpose2d_gemm


def _device() -> str:
    configured = os.getenv("WHISPER_DEVICE", "").strip()
    if not configured:
        configured = os.getenv("DEVICE", "").strip()
    if not configured:
        configured = "cuda"
    device = configured.lower()
    if device in ("gpu", "auto"):
        return "cuda"
    if device in ("cpu", "cuda", "mps"):
        return device
    return "cuda"


def _convert_words(words: list) -> list:
    return [
        {"text": w.get("word", ""), "start": w.get("start", 0.0), "end": w.get("end", 0.0)}
        for w in words or []
    ]


def _convert_segments(segments: list) -> list:
    return [
        {
            "text": seg.get("text", "").strip(),
            "start": seg.get("start", 0.0),
            "end": seg.get("end", 0.0),
            "words": _convert_words(seg.get("words", [])),
        }
        for seg in segments
    ]


def main() -> None:
    if "--device" in sys.argv:
        idx = sys.argv.index("--device")
        if idx + 1 < len(sys.argv):
            os.environ["WHISPER_DEVICE"] = sys.argv[idx + 1]

    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    force_cpu = "--cpu" in sys.argv

    if "--benchmark-load" in sys.argv:
        device = "cpu" if force_cpu else _device()
        if device != "cpu":
            _apply_conv_patch()
        import whisper
        model_name = os.getenv("WHISPER_MODEL", "large-v3-turbo")
        _ = whisper.load_model(model_name, device=device)
        print("[BENCHMARK_LOAD_DONE]")
        return

    if len(args) < 2:
        print(f"Usage: {sys.argv[0]} <vocals_wav> <session_path> [language] [--device cpu|cuda]", file=sys.stderr)
        sys.exit(1)

    vocals_file = Path(args[0])
    session_path = Path(args[1])
    language = None if args[2] == "auto" else args[2] if len(args) > 2 else None

    if not vocals_file.is_file():
        print(f"Error: vocals file not found: {vocals_file}", file=sys.stderr)
        sys.exit(1)

    device = "cpu" if force_cpu else _device()
    if device != "cpu":
        _apply_conv_patch()

    import whisper

    model_name = os.getenv("WHISPER_MODEL", "large-v3-turbo")
    model = whisper.load_model(model_name, device=device)

    result = model.transcribe(str(vocals_file), language=language, word_timestamps=False, verbose=False)

    segments = result.get("segments", [])
    converted = _convert_segments(segments)
    if not converted:
        print("Whisper did not return any segments.", file=sys.stderr)
        sys.exit(1)

    duration_ms = int(round(max(seg.get("end", 0) for seg in segments) * 1000))

    metadata_dir = session_path / "metadata"
    metadata_dir.mkdir(parents=True, exist_ok=True)

    payload = {
        "audio_info": {"duration": duration_ms},
        "result": {
            "text": (result.get("text") or "").strip(),
            "segments": converted,
        },
        "_device": device,
        "detected_language": result.get("language", language or "unknown"),
    }

    output_file = metadata_dir / "asr.json"
    output_file.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"ASR_OUTPUT:{output_file}")


if __name__ == "__main__":
    main()
