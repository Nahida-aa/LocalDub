"""
Compute WER (word-level) and CER (character-level) between reference and hypothesis ASR JSON files.

Usage:
    python3 wer.py <ref.json> <hyp.json>
    python3 wer.py --text <ref_text> <hyp_text>

Output: JSON line with { wer, cer, ref_words, hyp_words, ref_chars, hyp_chars, substitutions, insertions, deletions }
"""
from __future__ import annotations

import json
import sys


def _levenshtein(ref: list[str], hyp: list[str]) -> tuple[int, int, int]:
    """Returns (substitutions+insertions+deletions, insertions, deletions)."""
    m, n = len(ref), len(hyp)
    d = [[0] * (n + 1) for _ in range(m + 1)]
    for i in range(m + 1):
        d[i][0] = i
    for j in range(n + 1):
        d[0][j] = j
    for i in range(1, m + 1):
        for j in range(1, n + 1):
            cost = 0 if ref[i - 1] == hyp[j - 1] else 1
            d[i][j] = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost)
    errors = d[m][n]

    # Backtrace to count S/I/D
    i, j = m, n
    subs = ins = dels = 0
    while i > 0 or j > 0:
        if i > 0 and j > 0 and ref[i - 1] == hyp[j - 1]:
            i -= 1
            j -= 1
        elif i > 0 and j > 0 and d[i][j] == d[i - 1][j - 1] + 1:
            subs += 1
            i -= 1
            j -= 1
        elif j > 0 and d[i][j] == d[i][j - 1] + 1:
            ins += 1
            j -= 1
        elif i > 0 and d[i][j] == d[i - 1][j] + 1:
            dels += 1
            i -= 1
    return errors, subs, ins, dels


def compute(ref_text: str, hyp_text: str) -> dict:
    ref_words = ref_text.split()
    hyp_words = hyp_text.split()
    ref_chars = list(ref_text.replace(" ", ""))
    hyp_chars = list(hyp_text.replace(" ", ""))

    word_err, word_subs, word_ins, word_dels = _levenshtein(ref_words, hyp_words)
    char_err, char_subs, char_ins, char_dels = _levenshtein(ref_chars, hyp_chars)

    return {
        "wer": round(word_err / max(len(ref_words), 1), 4),
        "cer": round(char_err / max(len(ref_chars), 1), 4),
        "ref_words": len(ref_words),
        "hyp_words": len(hyp_words),
        "ref_chars": len(ref_chars),
        "hyp_chars": len(hyp_chars),
        "word_errors": word_err,
        "word_subs": word_subs,
        "word_ins": word_ins,
        "word_dels": word_dels,
        "char_errors": char_err,
        "char_subs": char_subs,
        "char_ins": char_ins,
        "char_dels": char_dels,
    }


def main():
    if "--text" in sys.argv:
        idx = sys.argv.index("--text")
        ref_text = sys.argv[idx + 1]
        hyp_text = sys.argv[idx + 2]
    else:
        if len(sys.argv) < 3:
            print("Usage: python3 wer.py <ref.json> <hyp.json>", file=sys.stderr)
            sys.exit(1)
        with open(sys.argv[1], encoding="utf-8") as f:
            ref_data = json.load(f)
        with open(sys.argv[2], encoding="utf-8") as f:
            hyp_data = json.load(f)
        ref_text = ref_data["result"]["text"]
        hyp_text = hyp_data["result"]["text"]

    print(json.dumps(compute(ref_text, hyp_text), ensure_ascii=False))


if __name__ == "__main__":
    main()
