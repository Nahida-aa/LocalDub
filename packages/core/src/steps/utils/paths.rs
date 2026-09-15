//! 按 step 分命名空间的类型化路径词汇表。
//!
//! 两类东西：
//!
//! 1. **有构造函数的路径类型**（`{asr,sf_ocr,asr_ocr}_fix::SrtPath`）——**用这些**。
//!    字段私有，路径只能经 `SrtPath::new(video_dir)` 产生：拼法只有一个来源，
//!    且三种类型互不兼容（provenance 由类型系统保证，见 `srt_path_types_are_distinct`）。
//! 2. **裸别名 + 预留命名的 `*Path`**——尚无消费者，见本文件后半段的说明。
//!
//! 三个 `SrtPath` **各写一遍、不用宏生成**：三个 fix 步骤的产物结构并不相像
//! （`asr_fix` 2 个文件 / `sf_ocr_fix` 3 个 / `asr_ocr_fix` 11 个），只有
//! `srt.json` 这一个文件名偶然重合。用宏会把「目录名只写一次」的收益，换成
//! 三个类型全部失去跳转定义——而它们随时可能各自长出别的产物，届时宏会碍事。
//!
//! `dead_code` 在此**刻意放行**：后半部分是词汇表，未使用的条目不是疏漏而是待用。
//! （注意：`translate::LangPath` 转成 `pub mod` 后，编译器才开始检查本文件的
//! 其余条目——此前它们在私有模块里，分析被跳过。）

#![allow(dead_code)]

use std::path::{Path, PathBuf};

// video_source.mp4
type VideoSourcePath = PathBuf;

pub mod separate_after {
    use super::*;
    // separate_after/target_bgm.wav
    pub type TargetBgmPath = PathBuf;
    // separate_after/target_3_vocals_mixed.wav
    pub type Target3VocalsMixedPath = PathBuf;
}

mod asr {
    use super::*;
    // asr/asr.json
    pub type AsrPath = PathBuf;
}

/// 规范化字幕产物的共同契约 `{step}/srt.json`。
///
/// **为什么有它**：三个 `SrtPath`（`asr_fix` / `sf_ocr_fix` / `asr_ocr_fix`）
/// 方法签名逐字相同。特征把「三个类型都承诺这件事」写成一句话——将来加第四个
/// （或加一个方法）时，编译器会点名谁没跟上，而不是靠三份复制互相对齐。
///
/// **只有三个方法，没有 `AsRef<Path>` / `Display`**：全仓的调用点一律走
/// `as_path()`（产出方喂 `write_srt_json`、消费者喂 `read_srt_json`、错误信息
/// 用 `{p}` 时也是 `as_path().display()`）。加 `AsRef` / `Display` 是「别人
/// 可能会要」的猜测——而它们**无法用 blanket impl 统一**（孤儿规则：不能为
/// 「所有实现本特征的类型」实现 `AsRef<Path>`），只能手写三份或上宏。
/// 没有真实需求，就不该有这三个复制品。
///
/// **`new()` 留在里面**是刻意的矛盾权衡：返回 `Self` 的关联函数在 trait 里
/// 调用要写 `<T as SrtJsonPath>::new(..)`，但我们**从不**这样调——调用点一律走
/// 具体类型（`asr_fix::SrtPath::new(video_dir)`），此时编译器命中固有 impl，
/// trait 里的声明只起契约作用，不影响可读性。
///
/// **`dir()` 为什么也算契约**：它无 `self`、日常用得少，但它是 `new()` 的父目录，
/// 两者必须一致，放一起才能让「产出方建目录」与「消费者读文件」用同一套词汇。
/// 不一致由 `dir_is_parent_of_srt_path` 钉住。
pub trait SrtJsonPath: Sized {
    /// 构造：`{video_dir}/{step}/srt.json`。
    fn new(video_dir: impl AsRef<Path>) -> Self;

    /// 规范化字幕的完整路径。
    fn as_path(&self) -> &Path;

    /// 所属目录（产出方需要先 `ensure_dir` 再来写）。
    fn dir(video_dir: impl AsRef<Path>) -> PathBuf;
}

pub mod asr_fix {
    use super::*;
    // asr_fix/asr_fix.json（详细产物，**非**规范化字幕——规范化的是下面 SrtPath）
    pub type AsrFixPath = PathBuf;

    /// 规范化字幕产物 `asr_fix/srt.json`。
    ///
    /// 字段私有：路径只能经 [`SrtPath::new`] 构造，故拼法只有一个来源。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SrtPath(PathBuf);

    impl SrtJsonPath for SrtPath {
        /// `{video_dir}/asr_fix/srt.json`。
        ///
        /// **这是全仓唯一的拼法**。此前 `"srt.json"` 在 6 处各写一遍
        /// （3 个产出方的 `out_dir.join("srt.json")` + 消费者手拼的
        /// `video.join("asr_fix").join("srt.json")` + engine 事件登记），
        /// 改名要同时改 6 处，漏一处是运行时「文件不存在」。
        fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(video_dir.as_ref().join("asr_fix").join("srt.json"))
        }

        fn as_path(&self) -> &Path {
            &self.0
        }

        fn dir(video_dir: impl AsRef<Path>) -> PathBuf {
            video_dir.as_ref().join("asr_fix")
        }
    }
}

pub mod sf_ocr_fix {
    use super::*;

    /// 规范化字幕产物 `sf_ocr_fix/srt.json`。
    ///
    /// 与 `asr_fix::SrtPath` **形似但不同型**——两者不可互换，provenance 由类型
    /// 系统保证。详见 `asr_fix::SrtPath` 的说明。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SrtPath(PathBuf);

    impl SrtJsonPath for SrtPath {
        fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(video_dir.as_ref().join("sf_ocr_fix").join("srt.json"))
        }

        fn as_path(&self) -> &Path {
            &self.0
        }

        fn dir(video_dir: impl AsRef<Path>) -> PathBuf {
            video_dir.as_ref().join("sf_ocr_fix")
        }
    }
}

pub mod asr_ocr_fix {
    use super::*;

    /// 规范化字幕产物 `asr_ocr_fix/srt.json`。
    ///
    /// 与 `asr_fix::SrtPath` **形似但不同型**——两者不可互换，provenance 由类型
    /// 系统保证。详见 `asr_fix::SrtPath` 的说明。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SrtPath(PathBuf);

    impl SrtJsonPath for SrtPath {
        fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(video_dir.as_ref().join("asr_ocr_fix").join("srt.json"))
        }

        fn as_path(&self) -> &Path {
            &self.0
        }

        fn dir(video_dir: impl AsRef<Path>) -> PathBuf {
            video_dir.as_ref().join("asr_ocr_fix")
        }
    }
}

pub mod translate {
    use super::*;
    // translate/translation.{lang}.json
    pub type LangPath = PathBuf;
}

// ---------------------------------------------------------------------------
// 以下为**尚无消费者的预留命名**（见文件头说明）。
// 读到它们时不要假设「有人在用」——实际路径拼装仍在 `utils/mod.rs`。
// ---------------------------------------------------------------------------

mod split_audio {
    use super::*;
    // split_audio/split_audio.json
    pub type SplitAudioPath = PathBuf;
    // split_audio/timings.json
    pub type TimingsPath = PathBuf;
}
mod tts {
    use super::*;
    // tts/tts.json
    pub type TtsPath = PathBuf;
}
mod mix_audio {
    use super::*;
    // mix_audio/audio_dubbing.wav
    pub type DubbingPath = PathBuf;
    // mix_audio/timings.json
    pub type TimingsPath = PathBuf;
}

pub struct SeparateAfterOutput {
    target_3_vocals_mixed_path: separate_after::Target3VocalsMixedPath,
    target_bgm_path: separate_after::TargetBgmPath,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个 fix 的 `srt.json` 路径形状：`{video_dir}/{step}/srt.json`。
    ///
    /// 字面钉死路径——构造函数是**全仓唯一拼法**，没有第二处旧写法可以对照，
    /// 所以这里必须充当那个对照物。改名时先红的是这条。
    ///
    /// 用 `as_path()` 取值而非 `to_string()`：这正是不给 `SrtPath` 实现
    /// `Display` 的代价与收益——少一个复制品，多一次 `.display()`。
    #[test]
    fn srt_path_shape_per_fix_step() {
        for (actual, expected) in [
            (asr_fix::SrtPath::new("/w").as_path().display().to_string(), "/w/asr_fix/srt.json"),
            (
                sf_ocr_fix::SrtPath::new("/w").as_path().display().to_string(),
                "/w/sf_ocr_fix/srt.json",
            ),
            (
                asr_ocr_fix::SrtPath::new("/w").as_path().display().to_string(),
                "/w/asr_ocr_fix/srt.json",
            ),
        ] {
            assert_eq!(actual, expected);
        }
    }

    /// `dir()` 必须是 `srt.json` 的父目录——产出方先用它 `ensure_dir` 再写。
    ///
    /// 不校验的话，两者可以各自漂移：`dir()` 指向 A、`new()` 落在 B，
    /// 表现为运行时「目录不存在」写失败。
    #[test]
    fn dir_is_parent_of_srt_path() {
        let p = asr_fix::SrtPath::new("/w");
        assert_eq!(p.as_path().parent(), Some(asr_fix::SrtPath::dir("/w").as_path()));
        assert_eq!(asr_fix::SrtPath::dir("/w"), PathBuf::from("/w/asr_fix"));
    }

    /// 三种 `SrtPath` 是**不同类型**，不可互换（provenance 由类型系统保证）。
    ///
    /// 这条是编译期事实的运行时留痕：若有人把三者退化成同一个类型别名
    /// （如都改成 `pub type SrtPath = PathBuf`），`TranslateInput` 的
    /// or-pattern 会重新编译通过，但「谁写的」就丢了。
    #[test]
    fn srt_path_types_are_distinct() {
        fn takes_asr(_: &asr_fix::SrtPath) {}
        takes_asr(&asr_fix::SrtPath::new("/w"));
        // 下一行若取消注释会编译失败——证明类型不可互换：
        // takes_asr(&sf_ocr_fix::SrtPath::new("/w"));
    }
}
