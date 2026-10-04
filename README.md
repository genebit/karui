<div align="center">
  <img src="src/app/icon.svg" width="72" alt="" />

  # karui <sub>軽い</sub>

  **Make videos lighter.**

  [![CI](https://img.shields.io/github/actions/workflow/status/genebit/karui/ci.yml?branch=master&label=CI&style=flat-square&color=04051B&labelColor=11141A)](https://github.com/genebit/karui/actions/workflows/ci.yml)
  [![Release](https://img.shields.io/github/v/tag/genebit/karui?sort=semver&label=release&style=flat-square&color=04051B&labelColor=11141A)](https://github.com/genebit/karui/releases)
  [![License](https://img.shields.io/badge/license-MIT-04051B?style=flat-square&labelColor=11141A)](LICENSE)
  [![Rust](https://img.shields.io/badge/rust-1.82%2B-04051B?style=flat-square&labelColor=11141A)](rust-toolchain.toml)

  <br />

  <img src="https://skillicons.dev/icons?i=rust,ts,react,nextjs,tauri,tailwind" alt="Rust, TypeScript, React, Next.js, Tauri, Tailwind CSS" />
</div>

<br />

*Karui* (軽い) is Japanese for *light*. Drop in videos or whole folders, pick
how small you want them, and karui re-encodes each one with ffmpeg to H.265 or
H.264 MP4. It shows progress per file and tells you how much space each one
saved.

- **Files or folders.** Drag them onto the window, or pass them on the command
  line. Folders are read one level deep, and karui skips non-videos and its own
  earlier outputs.
- **Safe outputs.** It never writes over a source. Outputs go beside each
  original as `name-compressed.mp4`, or into a folder you choose, and are
  numbered instead of replacing an existing file. An interrupted encode leaves
  nothing behind, and one that comes out no smaller than the original beside
  it is discarded.
- **Sensible defaults.** H.265 at CRF 28 with the `hvc1` tag, so QuickTime,
  Safari, and iPhones play it. Portrait phone footage, odd dimensions, 10-bit
  HDR, interlaced camcorder footage, and PCM audio are all handled. AAC that is
  already small is copied rather than encoded twice, and mono gets 64 kb/s.
- **Caps, not conversions.** "Up to 30 fps" and "720p" only reduce. A 24 fps
  film stays at 24 fps, and a 480p clip is not scaled up.

## Prerequisites

karui drives **ffmpeg**, which must be installed separately:

| Platform | Install |
|---|---|
| macOS | `brew install ffmpeg` |
| Debian / Ubuntu | `sudo apt install ffmpeg` |
| Arch | `sudo pacman -S ffmpeg` |
| Windows | `winget install ffmpeg` |

karui searches `PATH` plus the usual install locations (`/opt/homebrew/bin`,
`/usr/local/bin`, WinGet's links folder), so an app started from Finder or the
Start menu still finds it. `KARUI_FFMPEG` and `KARUI_FFPROBE` can name the
binaries directly.

To build from source you also need Rust from [rustup](https://rustup.rs)
(`rust-toolchain.toml` pins the channel), Node 22+, and pnpm 10.

## Quick start: the CLI

The engine needs no webview and no Node, so this works on a bare machine:

```bash
cargo build -p karui-cli --release

./target/release/karui doctor                      # where is ffmpeg, what can it encode
./target/release/karui compress ~/Movies/trip      # → trip/<name>-compressed.mp4
./target/release/karui compress a.mov b.mkv -o out --codec h264 --crf 24
./target/release/karui compress clips --max-res 720 --max-fps 30 --audio remove
./target/release/karui probe ~/Movies/trip         # what ffprobe reads from each file
```

| Flag | Default | |
|---|---|---|
| `-o, --output <dir>` | beside each source | Output folder. |
| `--codec h264\|h265` | `h265` | H.265 is about half the size; H.264 plays anywhere. |
| `--crf <0–51>` | 28 (h265), 23 (h264) | Lower is better quality and larger. |
| `--preset <name>` | `medium` | `ultrafast` … `veryslow`. Slower makes a smaller file at the same quality. |
| `--content general\|animation\|grain` | `general` | Tunes x264/x265 for animation and screen recordings, or to keep film grain (larger). |
| `--max-fps <n>` | source | Lower the frame rate only when the source is faster. |
| `--max-res <n>` | source | Cap the shorter side, e.g. `720`. |
| `--audio aac\|copy\|remove` | `aac` | AAC at up to 128 kb/s (64 for mono; AAC already that small is copied), keep as-is (when MP4 can hold it), or drop. |
| `--overwrite` | off | Replace existing outputs instead of numbering new ones. |
| `--bell` | off | Ring the terminal bell when the batch ends. |

Ctrl-C stops the current encode cleanly and deletes its partial file. The exit
code is `0` when every file succeeds, `1` when any fails, and `130` when the
batch was cancelled.

## Running the desktop app

On Linux, install the Tauri system dependencies once:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf   # Debian / Ubuntu
sudo pacman -S --needed base-devel webkit2gtk-4.1 gtk3 librsvg patchelf     # Arch
```

Then:

```bash
pnpm install
pnpm tauri dev        # dev server + window, hot reload
pnpm tauri build      # installers: AppImage, deb, dmg, msi, nsis
```

Shortcuts: <kbd>⌘O</kbd> adds videos, <kbd>⇧⌘O</kbd> adds a folder, and
<kbd>⌘↵</kbd> starts or stops the batch (<kbd>Ctrl</kbd> instead of <kbd>⌘</kbd>
on Linux and Windows). When a batch ends, the app plays the same three-tone
chime the original script did.

## Repository layout

| Path | Contents |
|---|---|
| `crates/karui-core` | The engine: discovery, ffprobe, output planning, ffmpeg arguments, encoding, progress. Pure Rust. |
| `crates/karui-cli` | The `karui` binary. |
| `src-tauri` | Desktop shell: IPC commands, batch runner, log bridge. Thin. |
| `src` | Next.js frontend (static export): settings sidebar, queue, output log. |

```
paths ─→ discover ─→ probe ─→ plan ─→ encode (ffmpeg) ─→ events ─┬─→ CLI progress line
                                                                 └─→ compress://event → UI
```

## Development

```bash
cargo test -p karui-core                                   # fast: no ffmpeg needed
cargo test -p karui-core -- --include-ignored              # also encodes real clips
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
pnpm typecheck
```

Contributors should read [`CLAUDE.md`](CLAUDE.md) for the architectural rules.

## History

karui began as `video-compressor`, a Python script that shelled out to ffmpeg
with a hand-escaped command string, plus an unfinished Tkinter GUI. The rewrite
keeps its purpose, its folder workflow, and its finishing chime, and fixes what
it got wrong:

- **Shell injection and broken names.** Commands went through `os.system` with
  only spaces and parentheses escaped. A file name containing `'`, `&`, `;`, or
  `$` failed or ran as shell syntax. Now ffmpeg gets an argument vector, and
  paths carry a `file:` prefix so a colon in a name is not read as a protocol.
- **Every folder entry was treated as a video**, including subfolders,
  `.DS_Store`, and earlier outputs.
- **Re-runs hung.** Outputs went to `../Output` under the same name, so a
  second run stopped at ffmpeg's invisible `Overwrite? [y/N]` prompt.
- **Failures were ignored.** ffmpeg's exit code was never checked. The final
  report then crashed on the first missing output and hid every other result.
- **Input was not validated.** A CRF outside 0–51, a non-numeric frame rate, or
  a codec choice other than 0 or 1 crashed or produced ffmpeg errors. The CLI
  defaulted to CRF 26 and the GUI to 28.
- **A fixed `-r 25`** duplicated frames in 24 fps footage and resampled
  everything else.
- **H.265 MP4s did not play** in QuickTime, Safari, or iOS without the `hvc1`
  tag.
- **State was passed through files.** `videofiles.txt` and `outputfiles.txt`
  were written into the working directory and committed to the repo.
- **The chime only worked on Linux**, through SoX and ALSA, and printed an
  error everywhere else.
- **The GUI did nothing.** Its Start button was a `pass`, its fields were not
  bound to anything, and H.264 and H.265 were checkboxes, so both or neither
  could be ticked.

## License

MIT © 2026 Johcel Gene T. Bitara. See [`LICENSE`](LICENSE).
