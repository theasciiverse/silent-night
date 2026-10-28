# Silent Night

A snowy forest at night, in the terminal. Snow falls at three depths and
blows in the wind; gusts sweep across from the windward side, shake the
snow off the spruces and lift it off the open snowfield, and lay it down
again in the lee of the cabin and the trees, so the drifts build and creep
downwind. A log cabin with lit windows and smoke from the chimney, a
decorated spruce, stars, the moon, a faint aurora, and once in a while a
sleigh across the sky.

The sound is made as it plays, nothing recorded: wind in the trees that
rises and falls with the gusts on screen, a whistle in the strongest, an
owl now and then, snow dropping off the branches and the roof, and sleigh
bells while the sleigh is up.

    cargo run --release              # m turns the sound off and on, any other key quits
    cargo run --release -- -s 7      # the same forest, the weather starting the same
    cargo run --release -- --mute    # start with the sound off

Sound on Linux goes to the first of `pw-cat` (PipeWire), `paplay`
(PulseAudio), `aplay` (ALSA) and `ffplay` that is installed; on macOS and
Windows to the default output device. Without any of them it is a silent
night.

Truecolor when the terminal says so (`COLORTERM=truecolor` or `24bit`,
Windows Terminal, Windows), 256 colours otherwise. The forest is rebuilt
when the window is resized.

Session log: `RUST_LOG=debug silent-night 2>silent-night.log` (start,
every key and resize, gusts, falling snow, owl and sleigh, a snapshot each
second). To listen to the mix without a screen: `silent-night
--render-sound night.wav 120` writes two minutes of it.

## Play

With [av](https://asciiverse.sh), the Asciiverse client:

```
av install silent-night
av play silent-night
```

To build it yourself: `cargo run --release`.

