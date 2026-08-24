//! 段时间轴前后 padding (镜像 TS `packages/core/stages/06_split_audio/pad_segment.ts`)。

/// 带时间轴的段 (至少需要 start/end)。
pub trait SegmentBounds {
    fn start_ms(&self) -> u64;
    fn end_ms(&self) -> u64;
}

/// 给各段时间轴加前后 padding (默认前 100ms / 后 300ms), 避免切块时把语音截断。
///
/// 规则 (与 TS 一致):
/// - 每段独立计算 start/end 的 padding 量, 相邻段间空白 (gap) 越足越接满默认;
/// - gap 不足时按比例分摊 start/end, 保证不越过相邻段;
/// - minGap=50ms 之下的缝直接取中点, 避免与相邻段重叠。
/// 返回带 `split_start_ms` / `split_end_ms` 的新 Vec, 不修改原数组。
pub fn pad_segments<T: SegmentBounds>(
    segments: &[T],
    start_pad: u64,
    end_pad: u64,
) -> Vec<(u64, u64)> {
    if segments.is_empty() {
        return Vec::new();
    }
    let min_gap: u64 = 50;
    let total = start_pad + end_pad;

    let start_pad_at = |idx: usize| -> u64 {
        let orig = segments[idx].start_ms();
        if idx == 0 {
            return orig.saturating_sub(start_pad);
        }
        let prev_end = segments[idx - 1].end_ms();
        let gap = orig.saturating_sub(prev_end);
        if gap >= total + min_gap {
            return orig.saturating_sub(start_pad);
        }
        if gap > min_gap {
            let share = ((gap - min_gap) * start_pad) / total;
            return orig - share;
        }
        prev_end + gap / 2
    };

    let end_pad_at = |idx: usize| -> u64 {
        let orig = segments[idx].end_ms();
        if idx == segments.len() - 1 {
            return orig + end_pad;
        }
        let next_start = segments[idx + 1].start_ms();
        let gap = next_start.saturating_sub(orig);
        if gap >= total + min_gap {
            return orig + end_pad;
        }
        if gap > min_gap {
            let share = ((gap - min_gap) * end_pad) / total;
            return orig + share;
        }
        orig + gap / 2
    };

    segments
        .iter()
        .enumerate()
        .map(|(idx, _)| {
            let new_start = start_pad_at(idx);
            let new_end = end_pad_at(idx);
            (new_start, new_end)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Seg {
        s: u64,
        e: u64,
    }
    impl SegmentBounds for Seg {
        fn start_ms(&self) -> u64 {
            self.s
        }
        fn end_ms(&self) -> u64 {
            self.e
        }
    }

    #[test]
    fn first_segment_pads_backward() {
        let segs = vec![Seg { s: 500, e: 1500 }];
        let out = pad_segments(&segs, 100, 300);
        assert_eq!(out[0], (400, 1800));
    }

    #[test]
    fn large_gap_keeps_full_pad() {
        let segs = vec![Seg { s: 1000, e: 1100 }, Seg { s: 2000, e: 2100 }];
        let out = pad_segments(&segs, 100, 300);
        // idx0 next start 2000, gap 900 >= 400+50 -> +end_pad
        assert_eq!(out[0], (900, 1400));
        // idx1 prev end 1100, gap 900 -> -start_pad
        assert_eq!(out[1], (1900, 2400));
    }

    #[test]
    fn tiny_gap_takes_midpoint() {
        let segs = vec![Seg { s: 1000, e: 1010 }, Seg { s: 1020, e: 1030 }];
        let out = pad_segments(&segs, 100, 300);
        // gap 10 <= min_gap -> midpoint 1015
        assert_eq!(out[0].1, 1015);
        assert_eq!(out[1].0, 1015);
    }
}
