#!/usr/bin/env bash
# Gives a wallet account a passport with a missed patch deadline, so you can sign in on the
# page and click Collect yourself.
# Usage: MAKER=<your maker> REPORTER=<your reporter> ARBITER=<your cert> scripts/breach-for.sh <wallet-account.testnet>
#        (the three accounts come from scripts/setup-accounts.sh)
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib.sh
[ $# -eq 1 ] || { echo "usage: $0 <wallet-account.testnet>" >&2; exit 2; }
: "${MAKER:?set MAKER}" "${REPORTER:?set REPORTER}" "${ARBITER:?set ARBITER}"
RUN=$(date +%m%d%H%M%S); LINE="try-$RUN"; TOKEN="TRY-$RUN"
SBOM_HASH=$(shasum -a 256 sbom/acme-cam-s2-1.4.0.cdx.json | awk '{print $1}')
call "$MAKER" create_line '{"line_id":"'"$LINE"'","name":"Acme Cam S2","arbiter":"'"$ARBITER"'","patch_window_sec":30,"advisory_stake":"100000000000000000000000"}'
call "$MAKER" mint_unit '{"token_id":"'"$TOKEN"'","line_id":"'"$LINE"'","owner_id":"'"$1"'","sbom_sha256":"'"$SBOM_HASH"'","firmware":"1.4.0","support_until":'"$(( $(date +%s) + 31536000 ))"'}' "2 NEAR"
call "$REPORTER" file_advisory '{"line_id":"'"$LINE"'","reference":"CVE-2022-37434","component":"pkg:generic/zlib@1.2.11","severity":"Critical"}' "0.1 NEAR"
bold "In about 35 seconds, open this, sign in as $1 (claim-only key or wallet) and click Collect:"
echo "https://aditya-galaxy.github.io/sureplate/?token=$TOKEN"
