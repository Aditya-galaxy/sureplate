#!/usr/bin/env bash
# Creates five faucet-funded testnet accounts for running the demo yourself, and prints
# the env vars scripts/demo.sh needs. No deploy required: the demo uses the public
# contract at sureplate.testnet, where anyone may open a product line.
# Usage: scripts/setup-accounts.sh <prefix>     e.g.  scripts/setup-accounts.sh judge42
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 <prefix>" >&2; exit 2; }
for role in maker alice bob reporter cert; do
  near account create-account sponsor-by-faucet-service "$1-$role.testnet" \
    autogenerate-new-keypair save-to-legacy-keychain network-config testnet create >/dev/null
  echo "created $1-$role.testnet"
done
echo
echo "MAKER=$1-maker.testnet ALICE=$1-alice.testnet BOB=$1-bob.testnet REPORTER=$1-reporter.testnet ARBITER=$1-cert.testnet scripts/demo.sh"
