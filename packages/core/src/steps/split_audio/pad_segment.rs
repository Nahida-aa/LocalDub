//! 段时间轴前后 padding (镜像 TS `packages/core/steps/06_split_audio/pad_segment.ts`)。

/// 带时间轴的段 (至少需要 start/end)。
pub trait SegmentBounds {
    fn start_ms(&self) -> u32;
    fn end_ms(&self) -> u32;
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
    start_pad: u32,
    end_pad: u32,
) -> Vec<(u32, u32)> {
    if segments.is_empty() {
        return Vec::new();
    }
    let min_gap: u32 = 50;
    let total = start_pad + end_pad;

    let start_pad_at = |idx: usize| -> u32 {
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

    let end_pad_at = |idx: usize| -> u32 {
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
        s: u32,
        e: u32,
    }
    impl SegmentBounds for Seg {
        fn start_ms(&self) -> u32 {
            self.s
        }
        fn end_ms(&self) -> u32 {
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

    /// **中间段自身反向时，输出也会反向**（`start >= end`）。
    ///
    /// 两条 padding 路径用了**不同基准**：
    /// - `start_pad_at` 用本段 `start_ms`
    /// - `end_pad_at` 用本段 `end_ms` 去量「到下一段 start 的 gap」
    ///
    /// 段正常时两者一致；段反向时（`end < start`）后者的 gap 会被算得虚大，
    /// 于是给足 `end_pad`，而前者已把 start 减小 —— 结果 `start > end`。
    ///
    /// **上游数据反向才会触发**（本函数不做校验也不修正）。
    /// 下游 `split_audio/mod.rs` 有 `start_ms >= end_ms` 的检查，会写 44 字节
    /// 空 wav 占位——不崩，但那个段实际是空的。
    #[test]
    fn reversed_segment_yields_reversed_bounds() {
        let segs = vec![
            Seg { s: 1000, e: 2000 },
            Seg { s: 3000, e: 2100 }, // 反向：end < start
            Seg { s: 5000, e: 6000 },
        ];
        let out = pad_segments(&segs, 100, 300);
        // 前/后段正常
        assert!(out[0].0 < out[0].1);
        assert!(out[2].0 < out[2].1);
        // 中间段反向 —— 这是当前行为（记录事实，非断言其合理）
        assert!(
            out[1].0 >= out[1].1,
            "反向段的 padding 结果也反向: {:?}",
            out[1]
        );
    }

    /// 单段（极短甚至自身反向）**不会**反向——这是巧合的安全，不是设计保证。
    ///
    /// 单段时 `idx==0` 走 `orig - start_pad`、`idx==len-1` 走 `orig + end_pad`，
    /// 只有段本身反向超过 `start_pad + end_pad`（默认 400ms）才会翻转。
    /// 钉住它是为了：若将来改了首/末段分支，这条会红。
    #[test]
    fn single_short_segment_does_not_reverse() {
        for (s, e) in [(1000u32, 1100u32), (1000, 1000), (1000, 900)] {
            let segs = vec![Seg { s, e }];
            let out = pad_segments(&segs, 100, 300);
            assert!(out[0].0 < out[0].1, "单段 ({s},{e}) 不该反向: {:?}", out[0]);
        }
    }
}
