#!/usr/bin/env bash
# Does the SBOM file you were given match the hash locked in a passport?
# Usage: scripts/verify-sbom.sh <token_id> <sbom-file>
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib.sh
[ $# -eq 2 ] || { echo "usage: $0 <token_id> <sbom-file>" >&2; exit 2; }
onchain=$(view get_unit '{"token_id":"'"$1"'"}' | jq -r '.sbom_sha256 // empty')
[ -n "$onchain" ] || { echo "no passport '$1' on $CONTRACT" >&2; exit 1; }
local_hash=$(shasum -a 256 "$2" | awk '{print $1}')
echo "on-chain  $onchain"
echo "file      $local_hash"
if [ "$onchain" = "$local_hash" ]; then echo "MATCH: this is the SBOM the maker committed to for $1"; else echo "MISMATCH"; exit 1; fi
