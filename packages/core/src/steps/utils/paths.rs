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

use derive_more::{AsRef, Deref, DerefMut, Display, From, Into};
use std::path::{Path, PathBuf};
/// 原视频 `video_source.mp4`（任务级输入，由 import 产出）。
///
/// 与 [`AudioSourcePath`] 同款：位置不是消费者自己拼的，而是 import 阶段
/// **写进 ctx** 的（`ctx.video_source_path`）。两个入口对应两类调用者：
///
/// | 入口 | 谁用 |
/// | --- | --- |
/// | [`from_video_dir`](Self::from_video_dir) | **import 产出方**——它写 ctx 之前就得知道路径 |
/// | [`from_state`](Self::from_state) | **消费者**——读 import 落下的那个值 |
///
/// 前者是「约定拼法」（import 落点固定为 `{video_dir}/video_source.mp4`，
/// 见 `import/download.rs`），后者是「事实」。
#[derive(Debug, Clone, PartialEq, Eq, Display, AsRef)]
#[display("{}", _0.display())]
#[as_ref(forward)]
pub struct VideoSourcePath(PathBuf);

impl VideoSourcePath {
    /// 按约定拼：`{video_dir}/video_source.mp4`（import 的落点）。
    pub fn from_video_dir(video_dir: impl AsRef<Path>) -> Self {
        Self(video_dir.as_ref().join("video_source.mp4"))
    }

    /// 从 ctx 取 `ctx.video_source_path`（import 写入的路径）。
    ///
    /// 缺失 → Err（与 [`AudioSourcePath::from_state`] 同款语义：
    /// 没导入过就没有这个文件，无从猜测）。
    pub fn from_state(state: &crate::context::WorkflowState) -> anyhow::Result<Self> {
        state
            .video_source_path
            .clone()
            .map(PathBuf::from)
            .map(Self)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "video_source_path 未设置 (session {}); 请先运行 import",
                    state.workflow.video_dir
                )
            })
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// 原始音频 `audio_source.wav`（任务级输入，由 import 产出）。
///
/// **与 `SrtPath` 的区别**：它的位置不是从 `video_dir` 拼出来的，而是 import
/// 阶段**写进 ctx** 的（`ctx.audio_source_path`）。两个构造入口对应两类调用者：
///
/// | 入口 | 谁用 |
/// | --- | --- |
/// | [`from_video_dir`](Self::from_video_dir) | **import 产出方**——它写 ctx 之前就得知道路径 |
/// | [`from_state`](Self::from_state) | **消费者**——读 import 落下的那个值 |
///
/// 两者在正常流程下指向同一文件（import 落点固定为 `{video_dir}/audio_source.wav`，
/// 见 `import/download.rs`），但语义不同：前者是「约定拼法」，后者是「事实」。
/// 与 `LangPath` 的 `new` / `from_state` 同一模式。
#[derive(Debug, Clone, PartialEq, Eq, Display, AsRef)]
#[display("{}", _0.display())]
#[as_ref(forward)]
pub struct AudioSourcePath(PathBuf);

impl AudioSourcePath {
    /// 按约定拼：`{video_dir}/audio_source.wav`（import 的落点）。
    pub fn from_video_dir(video_dir: impl AsRef<Path>) -> Self {
        Self(video_dir.as_ref().join("audio_source.wav"))
    }

    /// 从 ctx 取 `ctx.audio_source_path`（import 写入的绝对路径）。
    ///
    /// 缺失 → Err（与 [`crate::steps::utils::video_source_path`] 同款语义：
    /// 没导入过就没有这个文件，无从猜测）。
    pub fn from_state(state: &crate::context::WorkflowState) -> anyhow::Result<Self> {
        state
            .audio_source_path
            .clone()
            .map(PathBuf::from)
            .map(Self)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "audio_source_path 未设置 (session {}); 请先运行 import",
                    state.workflow.video_dir
                )
            })
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

pub mod separate {
    use super::*;

    /// separate/target_3_vocals.wav（demucs 的 vocals stem，**原始**人声）。
    ///
    /// newtype 包裹 `PathBuf`：拼法只有 [`new`](Self::new) 一个来源，其余靠
    /// `Deref` / `DerefMut` 透传到 `PathBuf`（`exists()` / `display()` / `join()`
    /// 等直接可用），`From<PathBuf>` / `Into<PathBuf>` 用于与既有 `PathBuf` 互转。
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct Target3VocalsPath(PathBuf);

    impl Target3VocalsPath {
        /// 唯一拼法来源：`{video_dir}/separate/target_3_vocals.wav`。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("separate")
                    .join("target_3_vocals.wav"),
            )
        }

        /// 规范化路径（消费点如 [`crate::steps::split_audio::SplitAudioSourcePath::as_path`] 走这里）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }
    }
}

pub mod separate_after {
    use super::*;
    // separate_after/target_bgm.wav
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct TargetBgmPath(PathBuf);

    impl TargetBgmPath {
        /// 唯一拼法来源：`{video_dir}/separate_after/target_bgm.wav`
        /// （与 `crate::steps::utils::separate_after_bgm_path` 同款落点）。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("separate_after")
                    .join("target_bgm.wav"),
            )
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }
    }
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

// 动态 dynamical dynamic
pub mod translate {
    use super::*;

    /// 翻译产物路径 `translate/{lang}.json`。
    ///
    /// 字段私有：路径只能经 [`LangPath::new`] 或 [`LangPath::from_state`] 产生，
    /// 与三个 `SrtPath` 同款约束（拼法只有一个来源）。
    ///
    /// **两个构造入口，对应两类调用者**——这是刻意的分工：
    ///
    /// | 入口 | 谁用 | 参数来源 |
    /// | --- | --- | --- |
    /// | [`new`](Self::new)`(video_dir, lang)` | **翻译步骤** | 它手里已有一对 `(dir, lang)`：`lang` 是它自己 `resolve_language` 决策出来的 |
    /// | [`from_state`](Self::from_state)`(ctx)` | **后续步骤** | 读 `ctx.target_language`——那个**已落定的事实** |
    ///
    /// 为什么后续步骤不该走 `new` + `resolve_language`：`resolve_language` 是
    /// **现算**（读配置、可能推断），而 `ctx.target_language` 是**翻译步骤落盘的
    /// 事实**。两者现在恰好一致（纯函数 + 同输入），但那是巧合的稳定——推断逻辑
    /// 一改（如新增直通分支），中途重跑下游就会分叉。`from_state` 让下游只能读到
    /// 事实，读不到「另算一遍」的机会。
    ///
    /// **存 `lang` 而不只存路径**：下游不只需要路径，还要语言本身——`mix_video`
    /// 用 `lang` 命名中间 SRT 并选字体（`default_font`：非 zh → Arial）。
    /// 若只存路径，下游就得再从 ctx 取一次语言，那又是第二个来源。
    ///
    /// **文件名是 `{lang}.json` 而非 `translation.{lang}.json`**：目录已经是
    /// `translate/`，再叫 `translation.` 是重复。同时它让「这份文件属于哪个语言」
    /// 一眼可读（`translate/vi.json`）。
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct LangPath {
        lang: String,
        path: PathBuf,
    }

    impl LangPath {
        /// 由 `(video_dir, lang)` 构造：`{video_dir}/translate/{lang}.json`。
        ///
        /// **翻译步骤专用**——它是唯一有资格决定 `lang` 的地方。
        pub fn new(video_dir: impl AsRef<Path>, lang: &str) -> Self {
            Self {
                lang: lang.to_string(),
                path: video_dir
                    .as_ref()
                    .join("translate")
                    .join(format!("{lang}.json")),
            }
        }

        /// 由 ctx 构造，语言取 `ctx.target_language`（**唯一写入点**见
        /// [`crate::steps::utils::persist_target_language`]）。
        ///
        /// **后续步骤专用**：读已落定的事实，不重新解析。
        ///
        /// 缺失 → Err（与 `read_translation_result` 同款语义：目标语言未知时
        /// 无从知道该读哪个文件；正常流程里 translate 跑过就一定有值）。
        pub fn from_state(ctx: &crate::context::WorkflowState) -> anyhow::Result<Self> {
            let lang = ctx.target_language.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "ctx.target_language 未设置, 无法定位翻译产物 (task {}); 请先运行 translate",
                    ctx.workflow.video_dir
                )
            })?;
            Ok(Self::new(&ctx.workflow.video_dir, lang))
        }

        pub fn as_path(&self) -> &Path {
            &self.path
        }

        /// 目标语言——与 [`as_path`](Self::as_path) **同源同构**，
        /// 该路径里的 `{lang}` 就是它。
        pub fn lang(&self) -> &str {
            &self.lang
        }
    }

    /// 路径的字符串形式——错误信息里 `{lang_path}` 直接可用。
    ///
    /// 与 `SrtPath` 不同（那边没人需要），这里是**真实需求**：
    /// `TranslateResult::read` 与 `mix_video` 的错误都要带上它。
    impl std::fmt::Display for LangPath {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.path.display())
        }
    }
}

// ---------------------------------------------------------------------------
// 以下为**尚无消费者的预留命名**（见文件头说明）。
// 读到它们时不要假设「有人在用」——实际路径拼装仍在 `utils/mod.rs`。
// ---------------------------------------------------------------------------

pub mod split_audio {
    use super::*;
    // split_audio/split_audio.json
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct SplitAudioPath(PathBuf);

    impl SplitAudioPath {
        /// 唯一拼法来源：`{video_dir}/split_audio/split_audio.json`
        /// （与 `crate::steps::utils::split_audio_path` 同款落点）。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("split_audio")
                    .join("split_audio.json"),
            )
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }
    }
    // split_audio/timings.json
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct TimingsPath(PathBuf);

    impl TimingsPath {
        /// 唯一拼法来源：`{video_dir}/split_audio/timings.json`
        /// （与 `crate::steps::utils::split_audio_timings_path` 同款落点）。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("split_audio")
                    .join("timings.json"),
            )
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }
    }

    /// 逐段人声切片目录 `split_audio/vocals`（`{:04}.wav`，1-based）。
    ///
    /// 产出方 `split_audio::cut_vocal_segments`，消费方 `tts`（参考资料音）。
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct VocalsDir(PathBuf);

    impl VocalsDir {
        /// 唯一拼法来源：`{video_dir}/split_audio/vocals`。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("split_audio")
                    .join("vocals"),
            )
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }

        /// 第 `idx` 段人声切片：`{video_dir}/split_audio/vocals/{:04}.wav`（1-based）。
        ///
        /// 与 [`tts::WavsDir::segment_path`](crate::steps::utils::paths::tts::WavsDir::segment_path)
        /// 同款命名；此前该文件名在 `split_audio` 与 `tts` 各拼一遍。
        pub fn segment_path(&self, idx: usize) -> PathBuf {
            self.0.join(format!("{:04}.wav", idx + 1))
        }
    }
}

pub mod tts {
    use super::*;
    // tts/tts.json
    pub type TtsPath = PathBuf;
    // tts/wavs/{:04}.wav
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct WavsDir(PathBuf);

    impl WavsDir {
        /// 唯一拼法来源：`{video_dir}/tts/wavs`。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(Path::new(video_dir.as_ref()).join("tts").join("wavs"))
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }

        /// 第 `idx` 段 TTS 输出 wav：`{video_dir}/tts/wavs/{:04}.wav`（1-based）。
        ///
        /// 此前文件名 `{:04}.wav` 在 `mix_audio` 与 `tts` 各拼一遍（均 `i + 1`），
        /// 改名要同步两处。现在拼法只在这里，消费点走 [`segment_path`](Self::segment_path)。
        pub fn segment_path(&self, idx: usize) -> PathBuf {
            self.0.join(format!("{:04}.wav", idx + 1))
        }
    }

    // tts/ref_doubled/ref_{:04}_x2.wav + ref_{:04}_list.txt
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct RefDoubledDir(PathBuf);

    impl RefDoubledDir {
        /// 唯一拼法来源：`{video_dir}/tts/ref_doubled`。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(Path::new(video_dir.as_ref()).join("tts").join("ref_doubled"))
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }

        /// 第 `idx` 段翻倍后的参考音：`ref_{:04}_x2.wav`（1-based）。
        pub fn x2_path(&self, idx: usize) -> PathBuf {
            self.0.join(format!("ref_{:04}_x2.wav", idx + 1))
        }

        /// 第 `idx` 段翻倍用的 ffmpeg concat 列表：`ref_{:04}_list.txt`（1-based）。
        pub fn list_path(&self, idx: usize) -> PathBuf {
            self.0.join(format!("ref_{:04}_list.txt", idx + 1))
        }
    }
}
pub mod mix_audio {
    use super::*;
    // mix_audio/audio_dubbing.wav
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct DubbingPath(PathBuf);

    impl DubbingPath {
        /// 唯一拼法来源：`{video_dir}/mix_audio/audio_dubbing.wav`
        /// （与 `crate::steps::utils::dubbing_path` 同款落点）。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("mix_audio")
                    .join("audio_dubbing.wav"),
            )
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }
    }
    // mix_audio/timings.json
    #[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, From, Into)]
    pub struct TimingsPath(PathBuf);

    impl TimingsPath {
        /// 唯一拼法来源：`{video_dir}/mix_audio/timings.json`。
        pub fn new(video_dir: impl AsRef<Path>) -> Self {
            Self(
                Path::new(video_dir.as_ref())
                    .join("mix_audio")
                    .join("timings.json"),
            )
        }

        /// 本类型承诺的路径访问器（与 `SrtPath` / `Target3VocalsPath` 同签名）。
        pub fn as_path(&self) -> &Path {
            &self.0
        }
    }
}

pub struct SeparateAfterOutput {
    target_3_vocals_mixed_path: separate_after::Target3VocalsMixedPath,
    target_bgm_path: separate_after::TargetBgmPath,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `video_source.mp4` 的落点：`{video_dir}/video_source.mp4`（**无** `media/` 中间层）。
    ///
    /// 钉这条是因为仓里存在第二处写法 `cmd/check/mod.rs` 拼的是
    /// `{video_dir}/media/video_source.mp4`，与 import 实际落点
    /// （`import/download.rs`）不一致。`from_video_dir` 才是权威拼法。
    #[test]
    fn video_source_path_shape() {
        let p = VideoSourcePath::from_video_dir("/w");
        assert_eq!(p.as_path().display().to_string(), "/w/video_source.mp4");
        // Display 走 PathBuf，便于直接拼进 ffmpeg 参数
        assert_eq!(p.to_string(), "/w/video_source.mp4");
    }

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
            (
                asr_fix::SrtPath::new("/w").as_path().display().to_string(),
                "/w/asr_fix/srt.json",
            ),
            (
                sf_ocr_fix::SrtPath::new("/w")
                    .as_path()
                    .display()
                    .to_string(),
                "/w/sf_ocr_fix/srt.json",
            ),
            (
                asr_ocr_fix::SrtPath::new("/w")
                    .as_path()
                    .display()
                    .to_string(),
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
        assert_eq!(
            p.as_path().parent(),
            Some(asr_fix::SrtPath::dir("/w").as_path())
        );
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

    /// `LangPath::new` 的路径形状。
    ///
    /// 字面钉死——它是翻译产物的唯一拼法，没有第二处旧写法可对照。
    #[test]
    fn lang_path_shape() {
        assert_eq!(
            translate::LangPath::new("/w", "vi")
                .as_path()
                .to_string_lossy(),
            "/w/translate/vi.json"
        );
    }

    /// `LangPath::from_state` 读 `ctx.target_language`——**已落定的事实**。
    ///
    /// 后续步骤靠它避开 `resolve_language` 的现算：即使 `input.workflow.targetLang`
    /// 与 ctx 里的值不同，`from_state` 也必须以 ctx 为准（ctx 是翻译步骤写的）。
    #[test]
    fn lang_path_from_state_uses_persisted_fact() {
        use crate::context::read_ctx_from_value;

        let mk = |ctx_lang: Option<&str>| {
            let mut ctx = read_ctx_from_value(json!({
                "workflow": {"id":"t","video_dir":"/w","url":"http://e","source":"remote",
                         "status":"running","created_at":"2024-01-01T00:00:00Z"},
                // 配置说要 vi，但 ctx 的落定事实可能不同
                "input": {"workflow": {"targetLang": "vi"}}
            }))
            .unwrap();
            ctx.target_language = ctx_lang.map(String::from);
            ctx
        };

        // ctx 说 zh → 用 zh，**无视** input 里的 vi
        let p = translate::LangPath::from_state(&mk(Some("zh"))).unwrap();
        assert_eq!(p.as_path().to_string_lossy(), "/w/translate/zh.json");

        // 缺失 → 报错（不现算、不推断）
        let err =
            translate::LangPath::from_state(&mk(None)).expect_err("缺失 target_language 必须报错");
        let msg = err.to_string();
        assert!(
            msg.contains("target_language") && msg.contains("translate"),
            "错误应点名字段并提示先跑 translate, 实际: {msg}"
        );
    }
}
