#!/usr/bin/env python3
"""生成剧集上传清单 xlsx。

数据来源: out/<剧名>/<集数>.mp4
一行对应一部剧:
    剧名 | 集数(总集数) | 姓名 | 时长(全剧求和) | 上传链接(网盘)
上传链接列留空，供上传后手工/脚本回填。

用法:
    uv run --no-project --with openpyxl scripts/make-upload-sheet.py \
        --series "师尊带我炸修真,梨落忘川雪" --name 安安
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

from openpyxl import Workbook
from openpyxl.styles import Alignment, Font, PatternFill
from openpyxl.utils import get_column_letter

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_ROOT = REPO_ROOT / "out"

HEADERS = ["剧名", "集数", "姓名", "时长", "上传链接(网盘)"]
COLUMN_WIDTHS = [28, 8, 12, 10, 46]


def ffprobe_duration(path: Path) -> float | None:
    """读取视频时长(秒)，失败返回 None。"""
    try:
        res = subprocess.run(
            [
                "ffprobe",
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                str(path),
            ],
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        print("ffprobe 未找到，请先 ensure 环境", file=sys.stderr)
        return None
    if res.returncode != 0:
        return None
    try:
        sec = float(res.stdout.strip())
    except ValueError:
        return None
    return sec if sec > 0 else None


def format_duration(sec: float) -> str:
    total = round(sec)
    hours, rem = divmod(total, 3600)
    minutes, seconds = divmod(rem, 60)
    if hours:
        return f"{hours}:{minutes:02d}:{seconds:02d}"
    return f"{minutes:02d}:{seconds:02d}"


def episode_number(stem: str) -> int | None:
    m = re.search(r"\d+", stem)
    return int(m.group()) if m else None


def collect_episodes(series_dir: Path) -> list[tuple[int | None, str, Path]]:
    """返回 (集号, 显示用集数, 文件路径)，按集号升序。"""
    eps: list[tuple[int | None, str, Path]] = []
    for p in series_dir.glob("*.mp4"):
        num = episode_number(p.stem)
        eps.append((num, str(num) if num is not None else p.stem, p))
    eps.sort(key=lambda e: (e[0] is None, e[0] if e[0] is not None else 0, e[1]))
    return eps


def style_sheet(ws) -> None:
    header_font = Font(bold=True)
    header_fill = PatternFill("solid", fgColor="DDEBF7")
    for idx, title in enumerate(HEADERS, start=1):
        cell = ws.cell(row=1, column=idx, value=title)
        cell.font = header_font
        cell.fill = header_fill
        cell.alignment = Alignment(horizontal="center", vertical="center")
        ws.column_dimensions[get_column_letter(idx)].width = COLUMN_WIDTHS[idx - 1]
    ws.freeze_panes = "A2"
    ws.auto_filter.ref = f"A1:{get_column_letter(len(HEADERS))}{ws.max_row}"


def main() -> int:
    parser = argparse.ArgumentParser(description="生成剧集上传清单 xlsx")
    parser.add_argument(
        "--series",
        action="append",
        required=True,
        help="剧名，可重复或用逗号分隔，如 --series '师尊带我炸修真,梨落忘川雪'",
    )
    parser.add_argument("--name", required=True, help="姓名，如 安安")
    parser.add_argument("--root", default=str(DEFAULT_ROOT), help="剧集根目录 (默认: out)")
    parser.add_argument("--out", default=None, help="输出 xlsx 路径 (默认: out/上传清单_<姓名>.xlsx)")
    args = parser.parse_args()

    series_list: list[str] = []
    for raw in args.series:
        for item in raw.split(","):
            item = item.strip()
            if item and item not in series_list:
                series_list.append(item)

    root = Path(args.root).resolve()
    if not root.is_dir():
        print(f"剧集根目录不存在: {root}", file=sys.stderr)
        return 1

    wb = Workbook()
    ws = wb.active
    ws.title = "上传清单"

    row = 2
    total = 0
    missing: list[str] = []
    for series in series_list:
        series_dir = root / series
        if not series_dir.is_dir():
            missing.append(series)
            print(f"✗ 剧名目录不存在: {series_dir}", file=sys.stderr)
            continue
        episodes = collect_episodes(series_dir)
        if not episodes:
            print(f"✗ {series}: 没有 mp4 文件", file=sys.stderr)
            continue
        total_sec = 0.0
        failed = 0
        for _, _, path in episodes:
            sec = ffprobe_duration(path)
            if sec is None:
                print(f"  ! {series}/{path.name}: 读取时长失败", file=sys.stderr)
                failed += 1
                continue
            total_sec += sec
        ws.cell(row=row, column=1, value=series)
        ws.cell(row=row, column=2, value=len(episodes))
        ws.cell(row=row, column=3, value=args.name)
        ws.cell(row=row, column=4, value=format_duration(total_sec) if total_sec else "")
        ws.cell(row=row, column=5, value="")
        row += 1
        total += 1
        note = f", {failed} 集时长缺失" if failed else ""
        print(f"✓ {series}: {len(episodes)} 集, 总时长 {format_duration(total_sec)}{note}")

    if total == 0:
        print("没有任何剧集可写入，未生成文件", file=sys.stderr)
        return 1

    style_sheet(ws)

    out_path = Path(args.out) if args.out else root / f"上传清单_{args.name}.xlsx"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    wb.save(out_path)
    print(f"\n输出: {out_path} ({total} 行)")
    if missing:
        print(f"缺失剧名: {', '.join(missing)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
