//! 翻译阶段的**增量落盘** (`translate/translation.{lang}.partial.json`)。
//!
//! 为什么单独成模块: 这段是纯 I/O + 段组装, 没有业务逻辑, 可以独立测试;
//! 与 `out.rs`(类型) / `prompts.rs`(提示词) / `args.rs`(配置) 同构。
//!
//! 用途:
//! - **阶段内续跑**: 每译完一个 batch 就落盘, 中断后跳过已完成 batch
//! - **分析**: partial 里标了 `missing`, 能看到哪句一直翻不出来
//!
//! 正式结果写盘后 partial 会被删除 (见 `step_translate`)。

use std::path::Path;

use serde_json::Value;

use crate::steps::translate::out::{
    TranslatePartialResult, TranslatePartialSegment, TranslateResultMeta,
};

/// 每批句数。
///
/// **注意**: 这个值同时决定了 `batch_index = gi / BATCH_SIZE`, 也就是
/// partial 里段的归组方式。以前 `step_translate` 与 `write_partial` 各写了一个
/// 50, 一旦只改一处, 续跑时段的 batch 归属就会错位 (静默错误)。
pub const BATCH_SIZE: usize = 50;

/// 读 partial: 返回已完成 batch 集合与已记录的段。
///
/// 文件不存在 / 解析失败都当作"没有进度"——partial 只是加速与诊断用的
/// 中间态, 丢了重跑即可, 不该让 step 失败。
pub fn read_partial(path: &Path) -> (std::collections::HashSet<usize>, Vec<TranslatePartialSegment>) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (Default::default(), Vec::new());
    };
    let Ok(p) = serde_json::from_str::<TranslatePartialResult>(&raw) else {
        return (Default::default(), Vec::new());
    };
    (p.completed_batches.into_iter().collect(), p.segments)
}

/// 写 partial: 把 `dsts` (Some=已译, None=缺失) 与 `completed` 落盘。
///
/// `missing` 的判定: 该句没译出 **且** 所属 batch 未标记完成。
/// 已完成 batch 里的空译文按"已处理"算 (可能是原文就空), 不算缺失。
pub fn write_partial(
    path: &Path,
    texts: &[String],
    srt_segments: &[Value],
    dsts: &[Option<String>],
    completed: &std::collections::HashSet<usize>,
    src_lang: &str,
    target_lang: &str,
) -> anyhow::Result<()> {
    let segments: Vec<TranslatePartialSegment> = (0..texts.len())
        .map(|gi| {
            let bi = gi / BATCH_SIZE;
            let dst = dsts.get(gi).cloned().flatten().unwrap_or_default();
            let missing = dsts.get(gi).map(|o| o.is_none()).unwrap_or(true) && !completed.contains(&bi);
            TranslatePartialSegment {
                text: texts[gi].clone(),
                dst,
                src_lang: Some(src_lang.to_string()),
                dst_lang: Some(target_lang.to_string()),
                start_ms: srt_segments
                    .get(gi)
                    .and_then(|u| u.get("start_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                end_ms: srt_segments
                    .get(gi)
                    .and_then(|u| u.get("end_ms"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32,
                batch_index: bi,
                missing,
            }
        })
        .collect();
    let partial = TranslatePartialResult {
        segments,
        completed_batches: completed.iter().copied().collect(),
        meta: TranslateResultMeta {
            src_lang: src_lang.to_string(),
            target_lang: target_lang.to_string(),
        },
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("创建 {} 失败: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(&partial)
        .map_err(|e| anyhow::anyhow!("序列化 partial 失败: {e}"))?;
    std::fs::write(path, json)
        .map_err(|e| anyhow::anyhow!("写入 {} 失败: {}", path.display(), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segs(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| serde_json::json!({"start_ms": i * 100, "end_ms": i * 100 + 50}))
            .collect()
    }

    /// `batch_index` 必须由 `BATCH_SIZE` 推导——这个常量是 partial 格式的一部分。
    #[test]
    fn batch_index_derives_from_batch_size() {
        let dir = std::env::temp_dir().join(format!("ld_tpart_idx_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.json");

        // 120 句 → 3 个 batch (50/50/20)
        let texts: Vec<String> = (0..120).map(|i| format!("T{i}")).collect();
        let dsts: Vec<Option<String>> = (0..120).map(|i| Some(format!("D{i}"))).collect();
        let completed: std::collections::HashSet<usize> = [0].into_iter().collect();
        write_partial(&path, &texts, &segs(120), &dsts, &completed, "zh", "vi").unwrap();

        let (done, out) = read_partial(&path);
        assert_eq!(out.len(), 120);
        assert_eq!(out[0].batch_index, 0);
        assert_eq!(out[49].batch_index, 0);
        assert_eq!(out[50].batch_index, 1, "50 之后应进入 batch 1");
        assert_eq!(out[99].batch_index, 1);
        assert_eq!(out[119].batch_index, 2);
        assert_eq!(done, completed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 缺失标注: 未译出且所属 batch 未完成 → missing=true。
    #[test]
    fn missing_only_when_untranslated_and_batch_incomplete() {
        let dir = std::env::temp_dir().join(format!("ld_tpart_miss_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.json");

        let texts: Vec<String> = vec!["a".into(), "b".into()];
        // 句 0 已译, 句 1 缺失
        let dsts: Vec<Option<String>> = vec![Some("A".into()), None];
        let completed: std::collections::HashSet<usize> = Default::default();
        write_partial(&path, &texts, &segs(2), &dsts, &completed, "zh", "vi").unwrap();

        let (_, out) = read_partial(&path);
        assert!(!out[0].missing && out[0].dst == "A");
        assert!(out[1].missing && out[1].dst.is_empty());

        // 同一份 dsts, 但 batch 0 标记完成 → 句 1 不再算缺失
        let completed: std::collections::HashSet<usize> = [0].into_iter().collect();
        write_partial(&path, &texts, &segs(2), &dsts, &completed, "zh", "vi").unwrap();
        let (_, out2) = read_partial(&path);
        assert!(!out2[1].missing, "已完成 batch 里的空译文不算缺失");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// partial 损坏/不存在 → 当作"无进度", 不报错。
    #[test]
    fn unreadable_partial_is_treated_as_no_progress() {
        let dir = std::env::temp_dir().join(format!("ld_tpart_bad_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.json");
        std::fs::write(&path, b"not json").unwrap();
        let (done, segs) = read_partial(&path);
        assert!(done.is_empty());
        assert!(segs.is_empty());

        let missing = dir.join("nope.json");
        let (done2, segs2) = read_partial(&missing);
        assert!(done2.is_empty() && segs2.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
