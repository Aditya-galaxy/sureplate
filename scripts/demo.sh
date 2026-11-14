#!/usr/bin/env bash
# Runs the whole Patchbond lifecycle against a live contract (testnet by default):
#   mint a bonded passport → advisory patched in time → resale → advisory missed → owner collects
#   → bogus advisory disputed and rejected → end of support → maker gets the rest back.
#
# Usage:  scripts/demo.sh            (uses the accounts below; override with env vars)
# Every run creates a fresh product line and unit, so it can be re-run any number of times.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib.sh

MAKER="${MAKER:-acme-cam.testnet}"
ALICE="${ALICE:-pb-alice.testnet}"
BOB="${BOB:-pb-bob.testnet}"
REPORTER="${REPORTER:-pb-reporter.testnet}"
ARBITER="${ARBITER:-pb-cert.testnet}"
WINDOW="${WINDOW:-40}"          # patch window, seconds (real lines would use days)
SUPPORT="${SUPPORT:-200}"       # support period, seconds from mint
BOND="${BOND:-2 NEAR}"
STAKE=100000000000000000000000  # 0.1 NEAR advisory stake
PAGE="${PAGE:-https://aditya-galaxy.github.io/patchbond/}"

RUN=$(date +%m%d%H%M%S)
LINE="cam-s2-$RUN"
TOKEN="CAM-S2-$RUN"
SBOM_FILE="sbom/acme-cam-s2-1.4.0.cdx.json"
SBOM_HASH=$(shasum -a 256 "$SBOM_FILE" | awk '{print $1}')
FIX_HASH=$(printf 'acme-cam-s2 firmware 1.4.1' | shasum -a 256 | awk '{print $1}')

show() {
  view get_unit '{"token_id":"'"$TOKEN"'"}' | jq -r '
    "  status \(.status)   owner \(.owner_id)   bond \((.bond|tonumber)/1e24*10000|round/10000) NEAR   remaining \((.remaining|tonumber)/1e24*10000|round/10000) NEAR",
    (.advisories[] | "    #\(.advisory.id) \(.advisory.reference) [\(.advisory.severity)] → \(.state)" + (if .state=="breach_claimable" then "  (owner can collect \((.claimable|tonumber)/1e24*10000|round/10000) NEAR)" else "" end))'
}
wait_until() { # unix seconds
  local left=$(( $1 - $(date +%s) + 3 ))
  if (( left > 0 )); then dim "  …waiting ${left}s for $2"; sleep "$left"; fi
}

bold "Patchbond demo · contract $CONTRACT · line $LINE · unit $TOKEN"
dim  "passport page: $PAGE?token=$TOKEN"

step "1. Maker opens a product line: ${WINDOW}s patch window, 0.1 NEAR advisory stake, arbiter $ARBITER"
call "$MAKER" create_line '{"line_id":"'"$LINE"'","name":"Acme Cam S2","arbiter":"'"$ARBITER"'","patch_window_sec":'"$WINDOW"',"advisory_stake":"'"$STAKE"'"}'

step "2. Maker ships a unit to Alice: mints its passport NFT and locks a $BOND bond inside it"
dim  "  SBOM $SBOM_FILE  sha256 $SBOM_HASH"
SUPPORT_UNTIL=$(( $(date +%s) + SUPPORT ))
call "$MAKER" mint_unit '{"token_id":"'"$TOKEN"'","line_id":"'"$LINE"'","owner_id":"'"$ALICE"'","sbom_sha256":"'"$SBOM_HASH"'","firmware":"1.4.0","support_until":'"$SUPPORT_UNTIL"'}' "$BOND"
show

step "3. Advisory A (High) is filed. Maker patches inside the window → nobody is owed anything"
A=$(view advisory_count '{}')
call "$REPORTER" file_advisory '{"line_id":"'"$LINE"'","reference":"CVE-2022-0778","component":"pkg:generic/openssl@3.0.1","severity":"High"}' "0.1 NEAR"
call "$MAKER" publish_patch '{"advisory_id":'"$A"',"fix_firmware":"1.4.1","fix_sha256":"'"$FIX_HASH"'"}'
show

step "4. Alice sells the camera to Bob. The passport, and the bond inside it, go with it"
call "$ALICE" nft_transfer '{"receiver_id":"'"$BOB"'","token_id":"'"$TOKEN"'"}' "1 yoctoNEAR"

step "5. Advisory B (Critical) is filed. This time the maker does nothing"
B=$(view advisory_count '{}')
call "$REPORTER" file_advisory '{"line_id":"'"$LINE"'","reference":"CVE-2022-37434","component":"pkg:generic/zlib@1.2.11","severity":"Critical"}' "0.1 NEAR"
DEADLINE=$(view get_advisory '{"advisory_id":'"$B"'}' | jq -r .deadline)
dim "  Bob tries to collect before the deadline:"
expect_fail "$BOB" claim '{"token_id":"'"$TOKEN"'","advisory_id":'"$B"'}'
wait_until "$DEADLINE" "the patch deadline to pass"
show
BEFORE=$(balance "$BOB")
dim "  Alice (the seller) tries to collect:"
expect_fail "$ALICE" claim '{"token_id":"'"$TOKEN"'","advisory_id":'"$B"'}'
dim "  Bob (the current owner) collects:"
call "$BOB" claim '{"token_id":"'"$TOKEN"'","advisory_id":'"$B"'}'
sleep 2
bold "  Bob's balance: $BEFORE → $(balance "$BOB") NEAR"
dim "  Bob tries to collect twice:"
expect_fail "$BOB" claim '{"token_id":"'"$TOKEN"'","advisory_id":'"$B"'}'
call "$REPORTER" reclaim_stake '{"advisory_id":'"$B"'}'
show

step "6. Advisory C is bogus. Maker disputes (pays a fee), the arbiter rejects it, the reporter's stake is slashed"
C=$(view advisory_count '{}')
call "$REPORTER" file_advisory '{"line_id":"'"$LINE"'","reference":"CVE-0000-0000 (not in this firmware)","component":"pkg:generic/log4j@2.14.0","severity":"Critical"}' "0.1 NEAR"
call "$MAKER" dispute '{"advisory_id":'"$C"'}' "0.1 NEAR"
call "$ARBITER" resolve_dispute '{"advisory_id":'"$C"',"upheld":false,"note":"log4j is not in the SBOM for 1.4.0"}'
show

step "7. Support period ends. Maker takes back what is left of the bond"
dim "  Maker tries to release early:"
if (( $(date +%s) < SUPPORT_UNTIL )); then
  expect_fail "$MAKER" release '{"token_id":"'"$TOKEN"'"}'
fi
wait_until "$SUPPORT_UNTIL" "the support period to end"
BEFORE=$(balance "$MAKER")
call "$MAKER" release '{"token_id":"'"$TOKEN"'"}'
sleep 2
bold "  Maker's balance: $BEFORE → $(balance "$MAKER") NEAR"
show

printf '\n'
bold "Done. Passport: $PAGE?token=$TOKEN   ·   $EXPLORER/address/$CONTRACT"
