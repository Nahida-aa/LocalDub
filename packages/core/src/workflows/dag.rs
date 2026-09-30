//! 步骤级 DAG：**哪些步骤依赖哪些步骤**的唯一声明处（代码即 DAG）。
//!
//! 与产物级依赖清单（各 `<Step>Input`）的分工：`XxxInput` 声明**读哪些文件**，
//! 这里声明**等哪些步骤**。两份清单说的是同一件事的两个层面——
//! `SplitAudioInput` 读 translate 产物 + separate 的 vocals stem，
//! 本文件里 `split_audio` 的上游就是 `[translate, separate]`。改了 Input 的
//! 读集而忘了改这里（或反过来）目前靠人工对齐 + 防漂移测试
//! （[`sf_ocr_dub_edges_are_topo_consistent_with_serial_constant`]）。
//!
//! 为什么按 workflow 建图而不是全局静态表：依赖边**随配置变**——`translate`
//! 的上游随 subtitle_source 变（asr→asr_fix / sf_ocr→sf_ocr_fix），
//! `mix_video` 的上游随 output_mode 变（dub 读 mix_audio + separate_after 的
//! bgm；subtitle 只读 split_audio 时序）。全局表要么塞满条件分支，要么撒谎。
//! 每个 workflow 一张具体小图，边就是事实。

use anyhow::bail;
use std::collections::{BTreeMap, BTreeSet};

use crate::workflows::args::{OutputMode, StepName, SubtitleSource};

/// 一张**具体流程**的步骤依赖图：节点 = 要跑的步骤，
/// 边 = 「`v` 开跑前 `u` 必须 Success」。
#[derive(Debug, Clone)]
pub struct StepDag {
    /// 要跑的步骤（决定调度范围；不在此表里的步骤不会被并行路径触碰）。
    nodes: BTreeSet<StepName>,
    /// step → 它的**上游依赖**（全部 Success 才可开跑）。
    deps: BTreeMap<StepName, Vec<StepName>>,
}

impl StepDag {
    /// 构造并校验：边只引用图内节点、无自环、无环。
    ///
    /// 构造期把这些拦下来——DAG 写错了应该在定义处就炸，而不是等调度器
    /// 跑到一半挂死或漏跑步骤。
    pub fn new(
        nodes: impl IntoIterator<Item = StepName>,
        edges: impl IntoIterator<Item = (StepName, Vec<StepName>)>,
    ) -> anyhow::Result<Self> {
        let nodes: BTreeSet<StepName> = nodes.into_iter().collect();
        let mut deps: BTreeMap<StepName, Vec<StepName>> = BTreeMap::new();
        for (step, ups) in edges {
            if !nodes.contains(&step) {
                bail!("DAG 边引用了图外的步骤 {step}");
            }
            for up in &ups {
                if !nodes.contains(up) {
                    bail!("{step} 的依赖 {up} 不在图内");
                }
                if up == &step {
                    bail!("{step} 依赖自身");
                }
            }
            deps.insert(step, ups);
        }
        for n in &nodes {
            deps.entry(*n).or_default();
        }

        // 无环校验（Kahn）：全部出队 = 无环。
        let mut remaining: BTreeMap<StepName, usize> =
            deps.iter().map(|(s, ups)| (*s, ups.len())).collect();
        let mut dependents: BTreeMap<StepName, Vec<StepName>> = BTreeMap::new();
        for (step, ups) in &deps {
            for up in ups {
                dependents.entry(*up).or_default().push(*step);
            }
        }
        let mut queue: Vec<StepName> = remaining
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(s, _)| *s)
            .collect();
        let mut processed = 0;
        while let Some(s) = queue.pop() {
            processed += 1;
            for d in dependents.get(&s).into_iter().flatten() {
                let n = remaining.get_mut(d).unwrap();
                *n -= 1;
                if *n == 0 {
                    queue.push(*d);
                }
            }
        }
        if processed != nodes.len() {
            let cyclic: Vec<String> = remaining
                .iter()
                .filter(|(_, n)| **n > 0)
                .map(|(s, _)| s.as_str().to_string())
                .collect();
            bail!("步骤依赖存在环: {}", cyclic.join(" -> "));
        }

        Ok(Self { nodes, deps })
    }

    /// `dub × sf_ocr`（配音 + 关键帧 OCR 字幕提取）的 DAG。
    ///
    /// ```text
    /// separate ─┬─► separate_after ─────────────────────────────────────┐ (bgm)
    ///           │                                                       ▼
    ///           └────────────────► split_audio ─► tts ─► mix_audio ─► mix_video
    ///                          ▲                                    ▲
    ///   sf_ocr_pre ─► sf_ocr ─┴─► sf_ocr_fix ─► translate ──────────┘
    /// ```
    ///
    /// 边的依据（与各 `XxxInput` 的读集一一对应）：
    /// - `split_audio`：读 translate 产物（硬依赖）+ `separate/target_3_vocals.wav`
    ///   （[`crate::steps::split_audio::SplitAudioSourcePath::Vocals`] 优先）。dub 里
    ///   separate 必然启用，等它跑完再切块，避免切了 `audio_source.wav` 又整个扔掉。
    /// - `mix_video`：dub 分支读 `separate_after/target_bgm.wav`（BgmPath::Separate）。
    pub fn sf_ocr_dub() -> Self {
        use StepName::*;
        Self::new(
            [
                Separate,
                SeparateAfter,
                SfOcrPre,
                SfOcr,
                SfOcrFix,
                Translate,
                SplitAudio,
                Tts,
                MixAudio,
                MixVideo,
            ],
            [
                (Separate, vec![]),
                (SeparateAfter, vec![Separate]),
                (SfOcrPre, vec![]),
                (SfOcr, vec![SfOcrPre]),
                (SfOcrFix, vec![SfOcr]),
                (Translate, vec![SfOcrFix]),
                (SplitAudio, vec![Translate, Separate]),
                (Tts, vec![SplitAudio]),
                (MixAudio, vec![Tts]),
                (MixVideo, vec![MixAudio, SeparateAfter]),
            ],
        )
        .expect("sf_ocr_dub 的边表是静态字面量, 构造失败说明写错了")
    }

    /// 按输入解析本流程的 DAG。实验期只有 `sf_ocr_dub` 有图，其它组合**明确报错**
    /// （不静默回退串行——用户以为并行生效了实际没有，比报错更糟）。
    pub fn for_input(input: &crate::input::Input) -> anyhow::Result<Self> {
        let mode = OutputMode::from_input(input);
        let source = SubtitleSource::from_input(input).map_err(anyhow::Error::msg)?;
        match (mode, source) {
            (OutputMode::Dub, SubtitleSource::SfOcr) => Ok(Self::sf_ocr_dub()),
            (mode, source) => bail!(
                "workflow.parallel 实验模式目前只有 sf_ocr_dub 的 DAG 定义, 当前是 {} × {source}",
                mode.as_str()
            ),
        }
    }

    pub fn nodes(&self) -> &BTreeSet<StepName> {
        &self.nodes
    }

    /// `step` 的直接上游依赖。
    pub fn deps(&self, step: StepName) -> &[StepName] {
        self.deps.get(&step).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// `target` 及其**传递上游**（targetStep 语义：跑到该步骤为止 → 只调度这个子图）。
    pub fn with_target_closure(&self, target: StepName) -> anyhow::Result<BTreeSet<StepName>> {
        if !self.nodes.contains(&target) {
            bail!("targetStep {target} 不在当前 DAG 内");
        }
        let mut seen = BTreeSet::new();
        let mut stack = vec![target];
        while let Some(s) = stack.pop() {
            if !seen.insert(s) {
                continue;
            }
            stack.extend(self.deps(s).iter().copied());
        }
        Ok(seen)
    }

    /// `from` 的**传递下游**（continueFrom 语义：该步重跑 → 下游全部重置 pending）。
    /// 不含 `from` 本身（调用方自己把它加进重置集）。
    pub fn transitive_dependents(&self, from: StepName) -> BTreeSet<StepName> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(s) = stack.pop() {
            for (step, ups) in &self.deps {
                if ups.contains(&s) && seen.insert(*step) {
                    stack.push(*step);
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::utils::steps::DUB_SF_OCR_STEPS;

    #[test]
    fn new_rejects_bad_edges() {
        use StepName::*;
        // 边引用图外节点
        assert!(StepDag::new([Asr], [(Asr, vec![Translate])]).is_err());
        // 自环
        assert!(StepDag::new([Asr], [(Asr, vec![Asr])]).is_err());
        // 环
        assert!(StepDag::new([Asr, AsrFix], [(Asr, vec![AsrFix]), (AsrFix, vec![Asr])]).is_err());
        // 正常
        assert!(StepDag::new([Asr, AsrFix], [(AsrFix, vec![Asr])]).is_ok());
    }

    /// **防漂移**：串行常量 `DUB_SF_OCR_STEPS` 必须是 `sf_ocr_dub` DAG 的一个
    /// 拓扑序，且节点集相等。串行与并行跑的是**同一张图**——边的先后关系与
    /// 线性序列冲突（或图里多出/少了步骤）时，两边就不再是同一个流程了。
    #[test]
    fn sf_ocr_dub_edges_are_topo_consistent_with_serial_constant() {
        let dag = StepDag::sf_ocr_dub();
        let pos = |s: StepName| {
            DUB_SF_OCR_STEPS
                .iter()
                .position(|n| *n == s.as_str())
                .unwrap_or_else(|| panic!("{} 不在 DUB_SF_OCR_STEPS 里", s.as_str()))
        };

        let serial: BTreeSet<StepName> = DUB_SF_OCR_STEPS
            .iter()
            .map(|n| n.parse().unwrap())
            .collect();
        assert_eq!(dag.nodes(), &serial, "DAG 节点集与串行序列不一致");

        for step in dag.nodes() {
            for up in dag.deps(*step) {
                assert!(
                    pos(*up) < pos(*step),
                    "边 {up} -> {step} 与串行常量的先后顺序矛盾（两边不是同一个流程）"
                );
            }
        }
    }

    #[test]
    fn target_closure_and_dependents() {
        let dag = StepDag::sf_ocr_dub();
        use StepName::*;

        // targetStep=sf_ocr → 只跑 sf_ocr_pre + sf_ocr
        let sub = dag.with_target_closure(SfOcr).unwrap();
        assert_eq!(sub, BTreeSet::from([SfOcrPre, SfOcr]));

        // targetStep=mix_video → 全图（它是汇点）
        assert_eq!(
            dag.with_target_closure(MixVideo).unwrap(),
            dag.nodes().clone()
        );

        // 图外 target → 报错
        assert!(dag.with_target_closure(Asr).is_err());

        // continueFrom=separate → 该步的传递下游全部重置
        let deps = dag.transitive_dependents(Separate);
        assert_eq!(
            deps,
            BTreeSet::from([SeparateAfter, SplitAudio, Tts, MixAudio, MixVideo])
        );
        // translate 的下游不经过 separate
        assert_eq!(
            dag.transitive_dependents(SfOcrFix),
            BTreeSet::from([Translate, SplitAudio, Tts, MixAudio, MixVideo])
        );
    }
}
