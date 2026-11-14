#!/usr/bin/env bash
# Synthesises the narration in video/narration.json with Google Cloud Text-to-Speech (Chirp 3 HD).
# Needs: gcloud logged in, a project with texttospeech.googleapis.com enabled (GCP_PROJECT or gcloud default), jq.
set -euo pipefail
cd "$(dirname "$0")"
PROJECT="${GCP_PROJECT:-$(gcloud config get-value project 2>/dev/null)}"
TOKEN=$(gcloud auth print-access-token)
VOICE=$(jq -r .voice narration.json)
mkdir -p voice
for key in $(jq -r '.lines | keys[]' narration.json); do
  jq -n --arg t "$(jq -r --arg k "$key" '.lines[$k]' narration.json)" --arg v "$VOICE" \
    '{input:{text:$t},voice:{languageCode:"en-US",name:$v},audioConfig:{audioEncoding:"LINEAR16",sampleRateHertz:48000}}' \
  | curl -sS -m 90 -X POST https://texttospeech.googleapis.com/v1/text:synthesize \
      -H "Authorization: Bearer $TOKEN" -H "x-goog-user-project: $PROJECT" -H 'Content-Type: application/json' -d @- \
  | jq -r '.audioContent // error("synthesis failed for " + $k)' --arg k "$key" | base64 -d > "voice/$key.wav"
  printf '%s  %ss\n' "$key" "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "voice/$key.wav")"
done
