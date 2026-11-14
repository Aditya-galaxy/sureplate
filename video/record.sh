#!/usr/bin/env bash
# Records the demo video: runs scripts/demo.sh for real and screenshots a composite page
# (terminal log + live passport page) with headless Chrome while it runs. Needs Chrome + ffmpeg.
set -uo pipefail
cd "$(dirname "$0")/.."
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
PORT=4174; OUT=video/out; rm -rf "$OUT"; mkdir -p "$OUT/frames"
export RUN="${RUN:-$(date +%m%d%H%M%S)}"; TOKEN="CAM-S2-$RUN"
python3 -m http.server "$PORT" >/dev/null 2>&1 & SRV=$!
trap 'kill $SRV 2>/dev/null' EXIT
sleep 1
shot() { # name url-query
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --window-size=1280,720 --virtual-time-budget=6000 \
    --screenshot="$OUT/frames/$1.png" "http://localhost:$PORT/video/composite.html?$2" >/dev/null 2>&1
}
shot intro card=intro; shot outro card=outro
: > "$OUT/demo.log"
scripts/demo.sh > "$OUT/demo.log" 2>&1 & DEMO=$!
i=0
while kill -0 $DEMO 2>/dev/null; do
  i=$((i+1)); shot "$(printf 'f%04d' $i)" "token=$TOKEN&log=out/demo.log"
done
for _ in 1 2 3; do i=$((i+1)); sleep 2; shot "$(printf 'f%04d' $i)" "token=$TOKEN&log=out/demo.log"; done
wait $DEMO; echo "demo exit=$? frames=$i token=$TOKEN"
# Assemble: cards held 4s, run frames at FRAME seconds each.
FRAME="${FRAME:-0.8}"
{ echo "file 'frames/intro.png'"; echo "duration 4.5"
  for f in "$OUT"/frames/f*.png; do echo "file 'frames/$(basename "$f")'"; echo "duration $FRAME"; done
  echo "file 'frames/outro.png'"; echo "duration 4.5"; echo "file 'frames/outro.png'"; } > "$OUT/list.txt"
ffmpeg -y -loglevel error -f concat -safe 0 -i "$OUT/list.txt" -vf "fps=30,format=yuv420p,scale=1280:720" \
  -c:v libx264 -crf 20 -movflags +faststart "$OUT/patchbond-demo.mp4" && ls -la "$OUT/patchbond-demo.mp4"
