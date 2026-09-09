use ocr_types::{edit_distance, FrameResult, OcrSegment};

/// fixOverlap implementation (mirrors TS `fixOverlap` in merge_frames.ts).
pub fn fix_overlap(
    asr_segs: &[OcrSegment],
    raw_frames: &[FrameResult],
    ocr_segs: &[OcrSegment],
    max_advance_ms: u64,
) -> Vec<OcrSegment> {
    let mut fix = asr_segs.to_vec();
    let mut sorted_frames = raw_frames.to_vec();
    sorted_frames.sort_by_key(|f| f.timestamp);

    // Step 1: boundary adjustment using raw frames
    for i in 1..fix.len() {
        let prev = fix[i - 1].clone();
        let cur = fix[i].clone();
        if cur.base.start_ms >= prev.base.end_ms {
            continue;
        }
        let overlap_end = prev.base.end_ms.min(cur.base.end_ms);
        for f in &sorted_frames {
            if f.timestamp < cur.base.start_ms {
                continue;
            }
            if f.timestamp > overlap_end {
                break;
            }
            let d_cur = edit_distance(&f.text, &cur.base.text);
            let d_prev = edit_distance(&f.text, &prev.base.text);
            if d_cur <= 2 && d_cur < d_prev {
                fix[i - 1].base.end_ms = f.timestamp;
                fix[i].base.start_ms = f.timestamp;
                break;
            }
        }
    }

    // Step 2: maxAdvanceMs check against ocrSegs
    for seg in &mut fix {
        let mut best_ocr: Option<&OcrSegment> = None;
        let mut best_overlap = 0;
        for o in ocr_segs {
            let overlap = if seg.base.start_ms == seg.base.end_ms {
                if seg.base.start_ms >= o.base.start_ms && seg.base.start_ms <= o.base.end_ms {
                    1
                } else {
                    0
                }
            } else {
                let ov = seg
                    .base
                    .end_ms
                    .min(o.base.end_ms)
                    .saturating_sub(seg.base.start_ms.max(o.base.start_ms));
                if ov > 0 {
                    ov
                } else {
                    0
                }
            };
            if overlap > best_overlap && edit_distance(&seg.base.text, &o.base.text) <= 2 {
                best_overlap = overlap;
                best_ocr = Some(o);
            }
        }
        if let Some(o) = best_ocr {
            if seg.base.start_ms + max_advance_ms < o.base.start_ms {
                seg.base.start_ms = o.base.start_ms;
            }
        }
    }

    // Filter zero-length segments
    fix.retain(|s| s.base.end_ms > s.base.start_ms);
    fix
}
