//! Hardware video encoders: the media engine in a Mac, or a GPU's encoder.
//!
//! x264 and x265 make the smallest files for a given look, but they keep
//! every core busy. A hardware encoder does the same job on dedicated silicon
//! several times faster and leaves the processor to decode, at some cost in
//! size. Decoding and filtering stay on the CPU either way: on Apple Silicon,
//! decoding to the GPU and copying frames back made a full encode five times
//! slower, and ffmpeg's `fps` and `scale` filters run on ordinary frames.
//!
//! What an ffmpeg build lists is not what works: `hevc_amf` is in every
//! Windows build whether or not an AMD driver is installed, and VAAPI needs a
//! render node the user can open. So each candidate is tried with a tiny
//! real encode, once per run of the program.

use crate::options::Codec;
use crate::tools::{command, Tools};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A probe encode that has not finished by now has hung in a driver.
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// One way of reaching a hardware encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// Apple's media engine, on every Apple Silicon and recent Intel Mac.
    Videotoolbox,
    /// Linux's video acceleration API: AMD and Intel GPUs through Mesa.
    Vaapi,
    /// NVIDIA GPUs, on Linux and Windows.
    Nvenc,
    /// AMD GPUs on Windows.
    Amf,
    /// Intel Quick Sync.
    Qsv,
}

impl Backend {
    /// The ffmpeg encoder for `codec`.
    pub fn encoder(self, codec: Codec) -> &'static str {
        match (self, codec) {
            (Backend::Videotoolbox, Codec::H264) => "h264_videotoolbox",
            (Backend::Videotoolbox, Codec::H265) => "hevc_videotoolbox",
            (Backend::Vaapi, Codec::H264) => "h264_vaapi",
            (Backend::Vaapi, Codec::H265) => "hevc_vaapi",
            (Backend::Nvenc, Codec::H264) => "h264_nvenc",
            (Backend::Nvenc, Codec::H265) => "hevc_nvenc",
            (Backend::Amf, Codec::H264) => "h264_amf",
            (Backend::Amf, Codec::H265) => "hevc_amf",
            (Backend::Qsv, Codec::H264) => "h264_qsv",
            (Backend::Qsv, Codec::H265) => "hevc_qsv",
        }
    }

    /// Whether the speed preset changes anything. Apple's and VAAPI's
    /// encoders have one speed.
    pub fn has_presets(self) -> bool {
        matches!(self, Backend::Nvenc | Backend::Amf | Backend::Qsv)
    }

    /// Tried in this order; the first that works is used. A discrete GPU's
    /// encoder comes before an integrated one's.
    fn candidates() -> &'static [Backend] {
        if cfg!(target_os = "macos") {
            &[Backend::Videotoolbox]
        } else if cfg!(windows) {
            &[Backend::Nvenc, Backend::Amf, Backend::Qsv]
        } else {
            &[Backend::Nvenc, Backend::Vaapi, Backend::Qsv]
        }
    }
}

/// A hardware encoder this machine has and that works. Mirrored by
/// `Hardware` in `src/lib/bindings.ts`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub backend: Backend,
    /// What to call it, e.g. "Apple VideoToolbox" or "AMD GPU (VA-API)".
    pub name: String,
    /// The codecs it encoded in the probe.
    pub codecs: Vec<Codec>,
    /// The VAAPI render node, e.g. `/dev/dri/renderD128`.
    #[serde(skip)]
    pub device: Option<PathBuf>,
}

impl Hardware {
    pub fn supports(&self, codec: Codec) -> bool {
        self.codecs.contains(&codec)
    }
}

/// The hardware encoder found on first use, for the life of the program.
/// Probing costs a few short encodes, and the hardware does not change.
pub fn detected(tools: &Tools) -> Option<Hardware> {
    static FOUND: OnceLock<Option<Hardware>> = OnceLock::new();
    FOUND.get_or_init(|| detect(tools)).clone()
}

/// Try each candidate for this platform and return the first that encodes.
pub fn detect(tools: &Tools) -> Option<Hardware> {
    let listing = command(&tools.ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();

    for &backend in Backend::candidates() {
        let devices = if backend == Backend::Vaapi {
            render_nodes().into_iter().map(Some).collect()
        } else {
            vec![None]
        };
        for device in devices {
            let codecs: Vec<Codec> = Codec::ALL
                .into_iter()
                .filter(|codec| listed(&listing, backend.encoder(*codec)))
                .filter(|codec| probe(tools, backend, *codec, device.as_deref()))
                .collect();
            if !codecs.is_empty() {
                let found = Hardware {
                    backend,
                    name: name(backend, device.as_deref()),
                    codecs,
                    device,
                };
                tracing::info!("Hardware encoding available: {}", found.name);
                return Some(found);
            }
        }
    }
    tracing::debug!("no working hardware encoder found");
    None
}

fn listed(listing: &str, encoder: &str) -> bool {
    listing
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(encoder))
}

/// Encode a few frames of a test pattern to nowhere with exactly the
/// arguments a real encode would use.
fn probe(tools: &Tools, backend: Backend, codec: Codec, device: Option<&Path>) -> bool {
    let hardware = Hardware {
        backend,
        name: String::new(),
        codecs: vec![codec],
        device: device.map(Path::to_path_buf),
    };
    let args = crate::args::probe_args(&hardware, codec);
    let Ok(mut child) = command(&tools.ffmpeg)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Linux render nodes, which VAAPI opens without a display server.
fn render_nodes() -> Vec<PathBuf> {
    let mut nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("renderD"))
                })
                .collect()
        })
        .unwrap_or_default();
    nodes.sort();
    nodes
}

fn name(backend: Backend, device: Option<&Path>) -> String {
    match backend {
        Backend::Videotoolbox => "Apple VideoToolbox".into(),
        Backend::Vaapi => {
            let vendor = device.and_then(vendor_of).map(vendor_name);
            format!("{} (VA-API)", vendor.unwrap_or("GPU"))
        }
        Backend::Nvenc => "NVIDIA NVENC".into(),
        Backend::Amf => "AMD AMF".into(),
        Backend::Qsv => "Intel Quick Sync".into(),
    }
}

/// The PCI vendor of a render node, from sysfs: `0x1002` is AMD.
fn vendor_of(device: &Path) -> Option<String> {
    let node = device.file_name()?;
    let path = Path::new("/sys/class/drm").join(node).join("device/vendor");
    std::fs::read_to_string(path)
        .ok()
        .map(|v| v.trim().to_string())
}

fn vendor_name(vendor: String) -> &'static str {
    match vendor.as_str() {
        "0x1002" => "AMD GPU",
        "0x8086" => "Intel GPU",
        "0x10de" => "NVIDIA GPU",
        _ => "GPU",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_names_follow_ffmpeg() {
        assert_eq!(Backend::Vaapi.encoder(Codec::H265), "hevc_vaapi");
        assert_eq!(
            Backend::Videotoolbox.encoder(Codec::H264),
            "h264_videotoolbox"
        );
        assert_eq!(Backend::Amf.encoder(Codec::H265), "hevc_amf");
    }

    #[test]
    fn listing_matches_whole_encoder_names() {
        let listing = " V....D hevc_videotoolbox    VideoToolbox H.265 Encoder (codec hevc)\n \
                       V....D hevc_vaapi_extra      not real\n";
        assert!(listed(listing, "hevc_videotoolbox"));
        assert!(!listed(listing, "hevc_vaapi"));
    }

    #[test]
    fn vaapi_names_the_gpu_vendor() {
        assert_eq!(vendor_name("0x1002".into()), "AMD GPU");
        assert_eq!(name(Backend::Vaapi, None), "GPU (VA-API)");
        assert_eq!(name(Backend::Videotoolbox, None), "Apple VideoToolbox");
    }

    #[test]
    fn serialises_for_the_window() {
        let hw = Hardware {
            backend: Backend::Vaapi,
            name: "AMD GPU (VA-API)".into(),
            codecs: vec![Codec::H264, Codec::H265],
            device: Some("/dev/dri/renderD128".into()),
        };
        let json = serde_json::to_value(&hw).expect("json");
        assert_eq!(json["backend"], "vaapi");
        assert_eq!(json["codecs"][1], "h265");
        assert!(json.get("device").is_none());
    }
}
