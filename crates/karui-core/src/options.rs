//! What a compression run is asked to do.
//!
//! Mirrored by `CompressOptions` in `src/lib/bindings.ts`. Every field has a
//! default, and `#[serde(default)]` means the frontend may omit any of them.

use crate::hardware::Hardware;
use crate::tools::Tools;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// The video encoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    /// Plays everywhere, including old phones and browsers.
    H264,
    /// Roughly half the size of H.264 at the same quality. The default, as it
    /// was in the original script.
    #[default]
    H265,
}

impl Codec {
    pub const ALL: [Codec; 2] = [Codec::H264, Codec::H265];

    /// The ffmpeg encoder name.
    pub fn encoder(self) -> &'static str {
        match self {
            Codec::H264 => "libx264",
            Codec::H265 => "libx265",
        }
    }

    /// The encoder's own documented default. The two scales differ: x265 at
    /// 28 looks roughly like x264 at 23, so one shared default would make one
    /// of them needlessly large or visibly worse.
    pub fn default_crf(self) -> u8 {
        match self {
            Codec::H264 => 23,
            Codec::H265 => 28,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::H265 => "h265",
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Codec {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().replace('.', "").as_str() {
            "h264" | "x264" | "avc" => Ok(Codec::H264),
            "h265" | "x265" | "hevc" => Ok(Codec::H265),
            other => Err(format!("unknown codec `{other}`; expected h264 or h265")),
        }
    }
}

/// Encoder speed against size. Slower presets find a smaller file at the
/// same quality; they never change the quality itself, which is `crf`'s job.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Ultrafast,
    Superfast,
    Veryfast,
    Faster,
    Fast,
    #[default]
    Medium,
    Slow,
    Slower,
    Veryslow,
}

impl Preset {
    pub const ALL: [Preset; 9] = [
        Preset::Ultrafast,
        Preset::Superfast,
        Preset::Veryfast,
        Preset::Faster,
        Preset::Fast,
        Preset::Medium,
        Preset::Slow,
        Preset::Slower,
        Preset::Veryslow,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Ultrafast => "ultrafast",
            Preset::Superfast => "superfast",
            Preset::Veryfast => "veryfast",
            Preset::Faster => "faster",
            Preset::Fast => "fast",
            Preset::Medium => "medium",
            Preset::Slow => "slow",
            Preset::Slower => "slower",
            Preset::Veryslow => "veryslow",
        }
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Preset {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let wanted = s.to_ascii_lowercase();
        Preset::ALL
            .into_iter()
            .find(|p| p.name() == wanted)
            .ok_or_else(|| format!("unknown preset `{s}`; expected ultrafast … veryslow"))
    }
}

/// What the footage is, so x264 and x265 can tune for it. Hardware encoders
/// have no such setting and ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    /// Camera footage. The encoders' own defaults.
    #[default]
    General,
    /// Cartoons, motion graphics, and screen recordings: flat colour and hard
    /// edges, which take stronger deblocking and more reference frames.
    Animation,
    /// Film grain or heavy sensor noise, kept rather than smeared. Larger.
    Grain,
}

impl Content {
    pub fn name(self) -> &'static str {
        match self {
            Content::General => "general",
            Content::Animation => "animation",
            Content::Grain => "grain",
        }
    }

    /// The `-tune` both x264 and x265 know by this name.
    pub fn tune(self) -> Option<&'static str> {
        match self {
            Content::General => None,
            Content::Animation => Some("animation"),
            Content::Grain => Some("grain"),
        }
    }
}

impl fmt::Display for Content {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Content {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "general" | "film" | "camera" => Ok(Content::General),
            "animation" | "screen" => Ok(Content::Animation),
            "grain" => Ok(Content::Grain),
            other => Err(format!(
                "unknown content `{other}`; expected general, animation, or grain"
            )),
        }
    }
}

/// What happens to the audio tracks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audio {
    /// AAC at no more than [`AAC_BITS_PER_SEC`], half that for mono. AAC
    /// already within it is copied rather than encoded a second time.
    #[default]
    Aac,
    /// Keep the original stream untouched, when MP4 can hold it.
    Copy,
    /// Drop every audio track.
    Remove,
}

/// Stereo AAC's bitrate: transparent for speech and most music, and small
/// next to any video stream.
pub const AAC_BITS_PER_SEC: u64 = 128_000;

impl Audio {
    pub fn name(self) -> &'static str {
        match self {
            Audio::Aac => "aac",
            Audio::Copy => "copy",
            Audio::Remove => "remove",
        }
    }
}

impl fmt::Display for Audio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Audio {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "aac" => Ok(Audio::Aac),
            "copy" => Ok(Audio::Copy),
            "remove" | "none" | "mute" => Ok(Audio::Remove),
            other => Err(format!(
                "unknown audio mode `{other}`; expected aac, copy, or remove"
            )),
        }
    }
}

/// What does the encoding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// x264 or x265 on the processor: the smallest file for a given look.
    #[default]
    Software,
    /// The machine's hardware encoder: several times faster, somewhat larger.
    Hardware,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Software => "software",
            Engine::Hardware => "hardware",
        }
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Engine {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "software" | "cpu" => Ok(Engine::Software),
            "hardware" | "gpu" => Ok(Engine::Hardware),
            other => Err(format!(
                "unknown encoder `{other}`; expected software or hardware"
            )),
        }
    }
}

/// The highest CRF either encoder accepts. Lower is better quality.
pub const CRF_MAX: u8 = 51;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompressOptions {
    pub codec: Codec,
    pub engine: Engine,
    /// The hardware encoder to use when `engine` is [`Engine::Hardware`].
    /// Never sent by the window: [`CompressOptions::resolved`] fills it in
    /// from what this machine has.
    #[serde(skip)]
    pub hardware: Option<Hardware>,
    /// `None` takes [`Codec::default_crf`].
    pub crf: Option<u8>,
    pub preset: Preset,
    pub content: Content,
    /// Frame-rate ceiling. A source already at or below it is left alone, so
    /// 24 fps film is never padded out to 30 with duplicated frames — which is
    /// what the original script's fixed `-r 25` did to anything slower.
    pub max_fps: Option<u32>,
    /// Ceiling on the **shorter** side, so `720` means 720p for portrait
    /// phone footage as well as landscape.
    pub max_resolution: Option<u32>,
    pub audio: Audio,
    /// `None` writes each output beside its input with
    /// [`crate::plan::OUTPUT_SUFFIX`] appended.
    pub output_dir: Option<PathBuf>,
    /// Where files read from a camera card go when `output_dir` is `None`.
    /// `None` takes [`crate::devices::default_import_dir`].
    pub import_dir: Option<PathBuf>,
    /// Replace an existing file at the output path rather than numbering.
    pub overwrite: bool,
}

impl CompressOptions {
    pub fn crf(&self) -> u8 {
        self.crf.unwrap_or_else(|| self.codec.default_crf())
    }

    /// These options with the hardware encoder filled in, when asked for.
    /// An error, rather than a quiet fall back to software, when this machine
    /// has none for the codec: the user chose speed and should hear why they
    /// are not getting it.
    pub fn resolved(&self, tools: &Tools) -> Result<CompressOptions> {
        let mut opts = self.clone();
        if opts.engine == Engine::Software || opts.hardware.is_some() {
            return Ok(opts);
        }
        match crate::hardware::detected(tools) {
            Some(hw) if hw.supports(opts.codec) => {
                opts.hardware = Some(hw);
                Ok(opts)
            }
            Some(hw) => Err(Error::Invalid(format!(
                "{} cannot encode {}; choose the other codec or software encoding",
                hw.name,
                opts.codec.name().to_uppercase()
            ))),
            None => Err(Error::Invalid(
                "this computer has no hardware video encoder that works with ffmpeg; \
                 use software encoding"
                    .into(),
            )),
        }
    }

    /// The hardware encoder in use, once resolved.
    pub fn hardware(&self) -> Option<&Hardware> {
        self.hardware
            .as_ref()
            .filter(|_| self.engine == Engine::Hardware)
    }

    pub fn import_dir_or_default(&self) -> Option<PathBuf> {
        self.import_dir
            .clone()
            .or_else(crate::devices::default_import_dir)
    }

    /// Reject anything ffmpeg would refuse, before a batch starts rather than
    /// once per file.
    pub fn validate(&self) -> Result<()> {
        if self.crf() > CRF_MAX {
            return Err(Error::Invalid(format!(
                "quality (CRF) must be 0–{CRF_MAX}, got {}",
                self.crf()
            )));
        }
        if let Some(fps) = self.max_fps {
            if !(1..=240).contains(&fps) {
                return Err(Error::Invalid(format!(
                    "frame rate must be 1–240, got {fps}"
                )));
            }
        }
        if let Some(res) = self.max_resolution {
            // 4:2:0 chroma needs both dimensions even.
            if !(16..=8640).contains(&res) || res % 2 != 0 {
                return Err(Error::Invalid(format!(
                    "resolution must be an even number of pixels from 16 to 8640, got {res}"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crf_follows_codec_until_set() {
        let mut opts = CompressOptions::default();
        assert_eq!(opts.crf(), 28);
        opts.codec = Codec::H264;
        assert_eq!(opts.crf(), 23);
        opts.crf = Some(30);
        assert_eq!(opts.crf(), 30);
    }

    #[test]
    fn validation_rejects_out_of_range_values() {
        let bad = [
            CompressOptions {
                crf: Some(52),
                ..Default::default()
            },
            CompressOptions {
                max_fps: Some(0),
                ..Default::default()
            },
            CompressOptions {
                max_resolution: Some(721),
                ..Default::default()
            },
        ];
        for opts in bad {
            assert!(opts.validate().is_err(), "{opts:?} should be rejected");
        }
        assert!(CompressOptions::default().validate().is_ok());
    }

    #[test]
    fn deserialises_partial_camel_case() {
        let opts: CompressOptions =
            serde_json::from_str(r#"{"codec":"h264","maxFps":30,"audio":"remove"}"#)
                .expect("parse");
        assert_eq!(opts.codec, Codec::H264);
        assert_eq!(opts.max_fps, Some(30));
        assert_eq!(opts.audio, Audio::Remove);
        assert_eq!(opts.preset, Preset::Medium);
    }

    #[test]
    fn parses_cli_spellings() {
        assert_eq!("H.265".parse::<Codec>(), Ok(Codec::H265));
        assert_eq!("hevc".parse::<Codec>(), Ok(Codec::H265));
        assert_eq!("VerySlow".parse::<Preset>(), Ok(Preset::Veryslow));
        assert_eq!("none".parse::<Audio>(), Ok(Audio::Remove));
        assert_eq!("screen".parse::<Content>(), Ok(Content::Animation));
        assert!("cartoon".parse::<Content>().is_err());
        assert!("vp9".parse::<Codec>().is_err());
    }
}
