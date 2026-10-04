---
name: verify
description: Run karui's full check suite — the same gates CI runs — and report what failed. Use after any change to the engine, the CLI, the Tauri shell, or the frontend, and before saying work is done.
---

# Verify karui

Run these from the repository root, in order, and stop to fix the first failure
before moving on. They mirror `.github/workflows/ci.yml`.

1. `cargo fmt --all --check` (if it fails, run `cargo fmt --all` and re-check)
2. `cargo clippy -p karui-core -p karui-cli --all-targets -- -D warnings`
3. `cargo test -p karui-core -p karui-cli`
4. If `ffmpeg -version` succeeds, also run
   `cargo test -p karui-core --test ffmpeg -- --include-ignored`. These tests
   encode real clips. If ffmpeg is missing, say so; do not report them as passed.
5. `pnpm typecheck`
6. `pnpm build`. This must run before step 7, because `generate_context!` reads `./out`.
7. `cargo clippy -p karui-app --all-targets -- -D warnings`

Skip steps 5–7 when only `crates/` changed, and skip steps 1–4 when only `src/`
changed. Say which steps you skipped.

For a change to ffmpeg arguments, also run one encode by hand and probe the
result:

```bash
ffmpeg -loglevel error -y -f lavfi -i testsrc2=size=1281x721:rate=60:duration=4 \
       -f lavfi -i sine=duration=4 -c:v libx264 -crf 10 -c:a aac "/tmp/karui v's:1.mov"
cargo run -q -p karui-cli -- -v compress "/tmp/karui v's:1.mov" --max-fps 30 --max-res 480
cargo run -q -p karui-cli -- probe "/tmp/karui v's:1-compressed.mp4"
```

Report each step's result, quoting any failure output verbatim.
