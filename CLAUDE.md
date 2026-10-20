# Silent Night

Our own demo for the Asciiverse catalog (kind `art`, id `silent-night`): a
snowy forest at night with wind, gusts, drifting snow, a faint aurora and
the forest's sound. Rust + crossterm, `src/main.rs` (scene) and
`src/sound.rs` (synthesis and output), MIT. Made 2026-10-20 for Christmas;
checked first whether a good terminal snow existed (snowmachine, its Go
port, rsnow, weathr: none had wind, drift or depth worth packaging).

- Remote: `origin` is github.com/theasciiverse/silent-night (private),
  branch `main`.
- Release: tag `v<version>-av.<n>` (first `v1.0.0-av.1`); the workflow
  builds static musl Linux, macOS and Windows on `*-av.*` tags.
- av starts it as `silent-night`; m toggles the sound, any other key,
  Ctrl+C, SIGTERM and SIGHUP quit; resize rebuilds; `-s SEED`.
- The scene runs WARMUP (480) seconds of weather before the first frame
  (about 0.1 s), so drifts and tree snow are already there.
- Drift: real mass moves (erosion above a packed crust, deposits in the
  lee of the cabin, trees and bumps, grains wrap at the screen edges so
  snow is conserved); in strong gusts massless spindrift adds the visible
  ground blizzard. Tune with `RUST_LOG=trace --render-sound`, which logs
  wind and drift each second and a snow profile every 30 s.
- Sound: Linux pipes s16le stereo 44.1 kHz to pw-cat (needs `--raw`),
  paplay, aplay or ffplay, falling through when one exits at once; macOS
  and Windows use cpal 0.18. Checked here: pw-cat plays, stops with the
  program; mix has no sub-30 Hz energy (35 Hz high-pass), peak about 0.45,
  wind -34 to -20 dB with the gusts, owl and bells 15 to 20 dB above the
  wind in their bands. Windows cross-compiles (gnu), not run; macOS not
  built here.
- Static build, in a copy of the tree: `docker run --rm --cpus 4 --memory
  8g -v "$PWD":/src -w /src rust:1-alpine sh -c 'apk add musl-dev && cargo
  build --release --locked -j2'`.
- Preview: `asciiverse/record-preview.sh target/release/silent-night
  asciiverse/silent-night.cast`.
- Session log: `RUST_LOG=debug silent-night 2>silent-night.log`.
