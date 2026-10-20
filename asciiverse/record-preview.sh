#!/bin/sh
# Records the catalog preview at 100x30 (hub/RECORDING.md), ended by
# SIGTERM, then caps it at 20 fps: the snow moves every frame, so the raw
# cast is large. Seed 14: aurora, drifts by the cabin, the sleigh across
# the aurora at about 18 s (the sleigh comes 14 s in; gusts and snowfall
# differ from run to run, since the weather follows the frame timing). No
# sound (a cast has none). Needs
# asciinema 3 and uv (for pyte).
#
#   asciiverse/record-preview.sh target/release/silent-night asciiverse/silent-night.cast [seconds]
set -eu
binary=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
out=$(pwd)/$2
seconds=${3:-30}
here=$(cd "$(dirname "$0")" && pwd)
home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
mkdir "$home/bin" && cp "$binary" "$home/bin/silent-night"
cd "$home"  # nothing in the recording names a local path
HOME="$home" PATH="$home/bin:$PATH" TERM=xterm-256color COLORTERM=truecolor LANG=C.UTF-8 \
asciinema rec --headless --overwrite -q \
  --window-size 100x30 --idle-time-limit 2 \
  --capture-env "TERM,COLORTERM,LANG" --title "Silent Night" \
  --command "timeout --foreground -s TERM $seconds silent-night -s 14 --no-sound" \
  "$home/raw.cast"
ls -l "$home/raw.cast" | awk '{print "raw cast: " $5 " bytes"}'
uv run -q --with pyte python -I "$here/cast-cap-fps.py" "$home/raw.cast" "$out" 20
