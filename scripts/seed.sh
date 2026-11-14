#!/usr/bin/env bash
# Seeds the long-lived showcase passports that the hosted page opens by default.
# Run once per contract. Leaves three units in three different states:
#   CAM-S2-0001  missed deadline, owner has NOT collected yet  → "breach claimable"
#   CAM-S2-0002  same breach, owner collected                  → "covered", bond half spent
#   BELL-0001    7-day patch window, advisory clock running    → "advisory open"
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib.sh

MAKER="${MAKER:-acme-cam.testnet}"; ALICE="${ALICE:-pb-alice.testnet}"; BOB="${BOB:-pb-bob.testnet}"
REPORTER="${REPORTER:-pb-reporter.testnet}"; ARBITER="${ARBITER:-sureplate-cert.sputnik-v2.testnet}"
STAKE=100000000000000000000000
SBOM_HASH=$(shasum -a 256 sbom/acme-cam-s2-1.4.0.cdx.json | awk '{print $1}')
FIX_HASH=$(printf 'acme-cam-s2 firmware 1.4.1' | shasum -a 256 | awk '{print $1}')
FIVE_YEARS=$(( $(date +%s) + 5*365*86400 ))

step "Line acme-cam-s2 (60s patch window) with two units"
call "$MAKER" create_line '{"line_id":"acme-cam-s2","name":"Acme Cam S2","arbiter":"'"$ARBITER"'","patch_window_sec":60,"advisory_stake":"'"$STAKE"'"}'
call "$MAKER" mint_unit '{"token_id":"CAM-S2-0001","line_id":"acme-cam-s2","owner_id":"'"$ALICE"'","sbom_sha256":"'"$SBOM_HASH"'","firmware":"1.4.0","support_until":'"$FIVE_YEARS"'}' "2 NEAR"
call "$MAKER" mint_unit '{"token_id":"CAM-S2-0002","line_id":"acme-cam-s2","owner_id":"'"$BOB"'","sbom_sha256":"'"$SBOM_HASH"'","firmware":"1.4.0","support_until":'"$FIVE_YEARS"'}' "2 NEAR"

step "Advisory patched in time"
A=$(view advisory_count '{}')
call "$REPORTER" file_advisory '{"line_id":"acme-cam-s2","reference":"CVE-2022-0778","component":"pkg:generic/openssl@3.0.1","severity":"High"}' "0.1 NEAR"
call "$MAKER" publish_patch '{"advisory_id":'"$A"',"fix_firmware":"1.4.1","fix_sha256":"'"$FIX_HASH"'"}'

step "Advisory the maker ignores"
B=$(view advisory_count '{}')
call "$REPORTER" file_advisory '{"line_id":"acme-cam-s2","reference":"CVE-2022-37434","component":"pkg:generic/zlib@1.2.11","severity":"Critical"}' "0.1 NEAR"
dim "  …waiting 66s for the deadline"; sleep 66
call "$BOB" claim '{"token_id":"CAM-S2-0002","advisory_id":'"$B"'}'
call "$REPORTER" reclaim_stake '{"advisory_id":'"$B"'}'

step "Line acme-doorbell (7-day patch window) with one unit and a running clock"
call "$MAKER" create_line '{"line_id":"acme-doorbell","name":"Acme Doorbell","arbiter":"'"$ARBITER"'","patch_window_sec":604800,"advisory_stake":"'"$STAKE"'"}'
call "$MAKER" mint_unit '{"token_id":"BELL-0001","line_id":"acme-doorbell","owner_id":"'"$ALICE"'","sbom_sha256":"'"$SBOM_HASH"'","firmware":"2.0.3","support_until":'"$FIVE_YEARS"'}' "1 NEAR"
call "$REPORTER" file_advisory '{"line_id":"acme-doorbell","reference":"CVE-2023-38545","component":"pkg:generic/curl@7.79.0","severity":"High"}' "0.1 NEAR"

bold "Seeded: CAM-S2-0001 (claimable), CAM-S2-0002 (collected), BELL-0001 (clock running)"
