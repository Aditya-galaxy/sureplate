#!/usr/bin/env bash
# Shared helpers for the Patchbond scripts. Source this file.
# Needs: near-cli-rs (`near`), curl, jq, python3. Keys are read from ~/.near-credentials (legacy keychain).

CONTRACT="${CONTRACT:-patchbond.testnet}"
RPC="${RPC:-https://test.rpc.fastnear.com}"
EXPLORER="${EXPLORER:-https://testnet.nearblocks.io}"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
dim()  { printf '\033[2m%s\033[0m\n' "$*"; }
step() { printf '\n\033[1;36m▸ %s\033[0m\n' "$*"; }

# Public RPCs rate-limit now and then: retry, alternating between two endpoints.
RPC_FALLBACK="${RPC_FALLBACK:-https://rpc.testnet.fastnear.com}"
_rpc() {
  local out url try
  for try in 1 2 3 4 5 6; do
    if (( try % 2 )); then url="$RPC"; else url="$RPC_FALLBACK"; fi
    out=$(curl -sS -m 30 "$url" -H 'Content-Type: application/json' -d "$1" 2>/dev/null) || out=""
    if printf '%s' "$out" | grep -q '"result"'; then printf '%s' "$out"; return 0; fi
    sleep "$try"
  done
  printf '%s' "$out"
}

# view <method> <json-args>  → prints the JSON the contract returned
view() {
  local args; args=$(printf '%s' "$2" | base64 | tr -d '\n')
  _rpc '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"optimistic","account_id":"'"$CONTRACT"'","method_name":"'"$1"'","args_base64":"'"$args"'"}}' \
    | python3 -c 'import sys,json
r=json.load(sys.stdin)
if "error" in r or "error" in r.get("result",{}): sys.exit("view failed: "+json.dumps(r)[:400])
print(bytes(r["result"]["result"]).decode())'
}

# balance <account>  → NEAR, 4 decimals
balance() {
  _rpc '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"view_account","finality":"optimistic","account_id":"'"$1"'"}}' \
    | python3 -c 'import sys,json; print("%.4f" % (int(json.load(sys.stdin)["result"]["amount"])/1e24))'
}

# near_amount <yocto>  → NEAR, 4 decimals
near_amount() { python3 -c 'import sys; print("%.4f" % (int(sys.argv[1])/1e24))' "$1"; }

# call <signer> <method> <json-args> [deposit, e.g. "2 NEAR"]
# Prints one line with the transaction link. Exits non-zero if the transaction failed.
call() {
  local signer="$1" method="$2" args="$3" deposit="${4:-0 NEAR}" out tx
  if ! out=$(near contract call-function as-transaction "$CONTRACT" "$method" json-args "$args" \
        prepaid-gas '60 Tgas' attached-deposit "$deposit" sign-as "$signer" \
        network-config testnet sign-with-legacy-keychain send 2>&1); then
    printf '\033[31m✗ %s.%s failed\033[0m\n%s\n' "$signer" "$method" "$(printf '%s' "$out" | sed 's/\x1b\[[0-9;]*m//g' | grep -i -E 'panicked|error|failed' | head -5)" >&2
    return 1
  fi
  tx=$(printf '%s' "$out" | sed 's/\x1b\[[0-9;]*m//g' | grep -o -E 'Transaction ID: [A-Za-z0-9]+' | head -1 | awk '{print $3}')
  printf '  \033[32m✓\033[0m %s → %s  \033[2m%s/txns/%s\033[0m\n' "$signer" "$method" "$EXPLORER" "$tx"
}

# expect_fail <signer> <method> <json-args> [deposit]  → succeeds only if the contract refuses the call
expect_fail() {
  local out
  if out=$(call "$@" 2>&1); then
    printf '\033[31m✗ expected the contract to refuse %s, but it went through\033[0m\n' "$2" >&2; return 1
  fi
  printf '  \033[32m✓\033[0m refused as expected: %s\n' "$(printf '%s' "$out" | grep -o -E "panicked[^\"]*|Smart contract panicked: [^\"]*" | head -1 | cut -c1-140)"
}
