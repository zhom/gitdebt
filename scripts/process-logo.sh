#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INPUT="${1:-$HOME/Downloads/gitdebt-logo.jpg}"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/gitdebt-logo.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

for tool in ffmpeg ffprobe potrace npx cargo; do
  command -v "$tool" >/dev/null || {
    echo "Missing required logo tool: $tool" >&2
    exit 1
  }
done

dimensions="$(
  ffprobe -v error -select_streams v:0 \
    -show_entries stream=width,height -of csv=s=x:p=0 "$INPUT"
)"
if [[ "$dimensions" != "1600x640" ]]; then
  echo "Expected the 1600x640 source image, got $dimensions" >&2
  exit 1
fi

# Preserve the supplied robot exactly:
#   1. crop its original black field to a centered 480x360 working canvas,
#   2. rotate the grayscale source 6 degrees counter-clockwise,
#   3. threshold after rotation so the trace receives only black/white pixels,
#   4. invert because Potrace traces black foreground on white.
ffmpeg -hide_banner -loglevel error -y -i "$INPUT" \
  -vf "crop=480:360:560:140,format=gray,rotate=-6*PI/180:ow=512:oh=512:c=black,lut=y='if(gte(val,128),255,0)',negate" \
  -frames:v 1 "$TMP/mark.pgm"

potrace -s --flat -t 4 -a 1 -O 0.2 \
  -o "$TMP/traced.svg" "$TMP/mark.pgm"
npx --yes svgo@4.0.2 --multipass \
  -i "$TMP/traced.svg" -o "$TMP/mark.svg"

perl -0pi -e \
  's/width="682\.667" height="682\.667"/width="512" height="512" role="img" aria-label="gitdebt robot"/' \
  "$TMP/mark.svg"

cp "$TMP/mark.svg" "$ROOT/assets/gitdebt-mark.svg"
cargo run --manifest-path "$ROOT/Cargo.toml" -p backend --example logo_assets
