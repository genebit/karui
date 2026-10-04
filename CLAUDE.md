# CLAUDE.md

This file provides guidance to Claude Code when working in this repository.

`karui` (軽い, "light") compresses videos with ffmpeg. You give it files or
folders, and it re-encodes each one to an H.265 or H.264 MP4 using a CRF,
preset, frame-rate cap, and resolution cap. It ships as a desktop app (Tauri)
and as a headless CLI that share one engine.

## Repository Layout

| Path | Contents |
|---|---|
| `crates/karui-core` | The engine. Pure Rust: discovery, ffprobe, output planning, ffmpeg argv, encoding, progress, batch events. No Tauri. |
| `crates/karui-cli` | Headless binary `karui`: `compress`, `probe`, `doctor`. |
| `src-tauri` | Desktop shell. IPC commands, batch runner, tracing bridge. Thin. |
| `src` | Next.js frontend (static export). Settings sidebar, queue, output log. |

## Commands

```bash
# Engine. Fast: no webview, no ffmpeg needed.
cargo test -p karui-core
cargo test -p karui-core -- --include-ignored   # + real encodes; needs ffmpeg on PATH
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all

# CLI.
cargo run -p karui-cli -- doctor
cargo run -p karui-cli -- compress <files or folders> [-o DIR] [--codec h264|h265] \
  [--crf N] [--preset P] [--content general|animation|grain] [--max-fps N] [--max-res N] \
  [--audio aac|copy|remove] [--overwrite]
cargo run -p karui-cli -- probe <files or folders>
cargo run -p karui-cli -- -vv compress ...      # debug: logs the exact ffmpeg argv

# Desktop app.
pnpm install
pnpm tauri dev                         # dev server + window, hot reload
pnpm build                             # next build -> ./out (static)
pnpm tauri build                       # production bundles
pnpm typecheck                         # tsc --noEmit

# The window also queues paths given on the command line.
./target/debug/karui-app ~/Movies/clips
```

A debug build loads the frontend from `devUrl` (`localhost:3000`), so it needs
`pnpm dev` running or it shows "Connection reset by peer". `pnpm tauri dev`
starts both. `cargo clippy -p karui-app` needs `pnpm build` first, because
`generate_context!` reads `../out`.

A quick sample clip for manual testing, with a name that exercises quoting:

```bash
ffmpeg -f lavfi -i testsrc2=size=1280x720:rate=60:duration=6 \
       -f lavfi -i sine=duration=6 -c:v libx264 -crf 10 -c:a aac "/tmp/a & b's clip.mov"
```

## Architecture

```
paths ─→ discover ─→ probe ─→ plan ─→ args ─→ encode ─→ batch::Event ─┬─→ CLI
                                                                      └─→ compress://event → UI
```

| Module | Responsibility |
|---|---|
| `core/src/discover.rs` | Files and folders → video paths. Filters by extension, skips hidden files and earlier outputs, dedupes. |
| `core/src/probe.rs` | ffprobe JSON → `MediaInfo`: duration, dimensions, rotation, fps, pix_fmt, streams. |
| `core/src/plan.rs` | Inputs → `Job { input, output }` for the whole batch, collision-free. |
| `core/src/args.rs` | `MediaInfo` + `CompressOptions` → ffmpeg argv. **Pure; every flag is unit-tested.** |
| `core/src/encode.rs` | Spawns ffmpeg, reads `-progress pipe:1`, handles cancellation, `.part` → final rename. |
| `core/src/batch.rs` | Runs jobs one after another and emits `Event`s to a callback. Shared by the CLI and the shell. |
| `core/src/preview.rs` | One frame before and after compression as same-size PNGs, plus SSIM/PSNR. Before a file is done, the "after" is a 24-frame sample encoded with the current settings, and the original still is reported first. Also the list's thumbnails: the keyframe a third of the way in, as a small JPEG. |
| `core/src/estimate.rs` | Time to compress: output pixels ÷ this machine's rate for the codec and preset. Rates come from a ~3 s benchmark, then from real encodes, and are saved in the app data dir. |
| `core/src/devices.rs` | Camera cards: finds mounted volumes with `DCIM` (or Sony/AVCHD folders), lists their videos recursively, and keeps the ledger of clips already imported. |
| `core/src/sizing.rs` | Estimated output size: encodes three 12-frame samples with the real settings and scales them up, charging keyframes at the encoder's interval. |
| `core/src/hardware.rs` | Hardware encoders: VideoToolbox (macOS), VA-API (Linux AMD/Intel), NVENC, AMF, Quick Sync. Probes each with a real encode once per run and keeps the first that works. |
| `core/src/tools.rs` | Finds ffmpeg/ffprobe, including the locations a GUI launch does not get on `PATH`. |
| `src-tauri/src/commands` | Argument marshalling and error mapping only. |
| `src/lib/queue.ts` | How batch events move queue rows between states. No decisions of its own. |

### Hard rules

- **NEVER** run ffmpeg or ffprobe through a shell, and never build a command
  string. Use `tools::command()` with an argument vector. This was the original
  script's worst bug: names containing `'`, `&`, or `$` broke or ran as shell
  syntax.
- **NEVER** pass a bare path to ffmpeg. Use `probe::file_url()` (`file:` +
  path), or a name containing `:` is read as a protocol.
- **NEVER** write directly to the final output path. ffmpeg writes
  `plan::partial_path()` (hidden `.name.mp4.part`). Rename only on success, and
  delete the partial on failure or cancel.
- **NEVER** let an output land on an input or on another job's output. All
  naming goes through `plan::plan`, which decides for the whole batch at once.
  Do not compute output paths anywhere else, including the frontend.
- **NEVER** put ffmpeg flags, discovery rules, or naming logic in `src-tauri`
  or `src`. Commands marshal arguments and map errors. If a command body grows
  past ~30 lines, the logic belongs in `core`.
- **NEVER** use a sync Tauri command for anything that spawns a process or
  touches many files. Sync commands run on the main thread and freeze the
  window. Use `async` with `spawn_blocking`, or a thread as
  `start_compression` does.
- **NEVER** run encodes in parallel. libx264 and libx265 already use every
  core, so two at once only split the machine.
- **NEVER** write to a camera card. Card files are read in place; with no
  output folder set, `plan::plan_for` sends them to the import folder
  (`~/Movies/karui` by default), not beside the source.
- **NEVER** upscale or upsample. `max_fps` and `max_resolution` are caps that
  apply only when the source exceeds them.
- **NEVER** commit on your own unless asked.
- **NEVER** include `Co-Authored-By` or 🤖 attribution in commits or pull
  request descriptions.
- **NEVER** add decorative section-separator comments. Use blank lines.
- **ALWAYS** run `cargo test -p karui-core` after touching the engine, and add
  an `args.rs` test for any new ffmpeg flag.

### ffmpeg details that are easy to get wrong

| Concern | Rule |
|---|---|
| H.265 playback on Apple | `-tag:v hvc1`. ffmpeg's default `hev1` will not play in QuickTime, Safari, or iOS. |
| x265 noise | `-x265-params log-level=error`. x265 ignores `-loglevel`. |
| Odd dimensions | 4:2:0 needs even width and height: `scale=trunc(iw/2)*2:trunc(ih/2)*2`. |
| Resolution cap | Applies to the **shorter** side, in display orientation (after rotation). `scale=-2:N` or `scale=N:-2`. |
| 10-bit / HDR | Keep `yuv420p10le` for H.265. H.264 gets 8-bit `yuv420p` and a note. |
| Audio copy | Only codecs MP4 can carry (`args::MP4_AUDIO`). Anything else is re-encoded to AAC with a note. |
| AAC bitrate | 128k, 64k for mono. AAC already within it (+5%, ffprobe's reading wanders) is copied, not re-encoded. Both only for a single audio track: `-c:a`/`-b:a` reach every track and only the first is probed. Decided once in `args::audio_out`, which `sizing` also reads. |
| Interlaced | `field_order` `tt`/`bb`/`tb`/`bt` → `bwdif=mode=send_frame` **first** in `-vf` (a scale before it blends the fields), one frame out per frame in, plus a note. |
| No smaller | An output beside its source that is no smaller is discarded before the rename (`Event::KeptOriginal`). One sent to an output folder or off a card is written regardless. |
| Cover art | Map the probed `video_stream` index, never `0:v:0`; an attached picture can be stream 0. |
| Hardware encoding | Only the encoder moves to hardware; decode and filters stay on the CPU (VideoToolbox decode plus a download was 5× slower). Probe with a real encode, since `-encoders` lists AMF and QSV without drivers. VA-API needs `-vaapi_device` before `-i` and `format=nv12,hwupload` last in `-vf`. Quality maps from the CRF slider: VideoToolbox `-q:v` was calibrated on an M2 to match x265's SSIM; the GPU QP mapping is a first guess. |
| Progress | `-progress pipe:1 -nostats`. `out_time_ms` is actually microseconds. Emit only on `progress=`. |
| Pipes | Drain stdout and stderr on separate threads, or ffmpeg deadlocks on a full stderr pipe. |
| stdin | `-nostdin` and `Stdio::null()`. ffmpeg otherwise reads the terminal for `q`. |
| Windows | `CREATE_NO_WINDOW` (in `tools::command`), or every spawn flashes a console. `rename` cannot replace, so remove first. |
| Ctrl-C | A terminal's SIGINT also reaches ffmpeg, which can exit first. A set cancel flag means cancelled, never failed. |
| App exit | Child processes outlive the app. `RunEvent::Exit` cancels and waits (`Runner::cancel_and_wait`). |

Notes, meaning decisions the user did not ask for, are returned from
`ffmpeg_args` in `EncodePlan::notes` and logged as warnings. Never change a
user's choice silently.

### Testing pattern

Unit tests live beside the code. `args.rs` asserts on the joined argv string.
`probe.rs` parses literal ffprobe JSON, including portrait phone, cover art,
and audio-only cases. `plan.rs` and `discover.rs` use scratch directories
under `std::env::temp_dir()`.

`core/tests/ffmpeg.rs` encodes clips that ffmpeg itself generates (`lavfi`
`testsrc2` + `sine`). The clip names contain a space, a quote, and a colon, and
one clip has odd dimensions. These tests are `#[ignore]` so `cargo test` passes
without ffmpeg; CI installs ffmpeg and runs `--include-ignored`. When you fix
an encode bug, reproduce it there first.

### Code style

**Rust**: edition 2021. `thiserror` for errors, never `anyhow` in `core`. No
`unwrap()`/`expect()` outside tests (`main`'s Tauri builder excepted). Lock
with `unwrap_or_else(PoisonError::into_inner)`. `src-tauri` exposes one
`AppError` that serialises to `{ kind, message, detail }`. Logging goes through
`tracing`, and a custom `Layer` forwards events to `log://line` for the UI log
panel.

Comments explain *why*: the player, file, or ffmpeg behaviour that forced the
decision.

**TypeScript**: `strict: true`. Prettier `{ singleQuote: true, semi: true,
tabWidth: 2, printWidth: 90 }`. Path alias `@/*`. `lucide-react` for icons.

`src/lib/bindings.ts` is hand-written and mirrors the `Serialize`/`Deserialize`
types in `karui-core` and `src-tauri` (camelCase). There is no codegen, so a
mismatch shows up at runtime. Change both in the same commit. The same goes for
`settings.ts::defaultCrf` (mirrors `Codec::default_crf`), `queue.ts::VIDEO_EXTENSIONS`
(mirrors `discover::VIDEO_EXTENSIONS`), and `utils.ts` formatting (mirrors
`core::units`, decimal units).

**Styling**: shadcn/ui (`radix-luma`, base colour `neutral`) on Tailwind v4,
configured in CSS via `@theme` in `src/app/globals.css` (no
`tailwind.config.js`). Components in `src/components/ui` are owned by this
repo, so edit them directly. Dark by default; clicking the version label
opens appearance settings (theme, font size as webview zoom, row density),
kept in `lib/appearance.ts` and applied before first paint by a script in
`app/layout.tsx`. Every colour must come from a theme token so both themes
work. The chrome is monochrome; colour is reserved for errors and
warnings, plus green for an estimated saving.

**Next.js**: static export only. No route handlers, middleware, server
actions, or `next/image` optimisation; anything that needs a Node server breaks
`tauri build`. `agentRules: false` in `next.config.ts` stops `next dev` from
appending its own block to this file.

**Commits**: Conventional Commits, lowercase, past tense, trailing period.
`feat(core): capped frame rate only when the source is faster.`
