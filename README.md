# Patchbond

**A device passport NFT that holds the maker's security-update bond.**

Built at NEAR HACKATHON (Nov 2026). Tracks: **NFTs That Do Something**, **Real-World Money**.

- Live contract (testnet): [`patchbond.testnet`](https://testnet.nearblocks.io/address/patchbond.testnet)
- Live passport page: https://aditya-galaxy.github.io/patchbond/
- One command runs the whole lifecycle on testnet: `scripts/demo.sh`

## The problem

Every connected device ships with a promise: "we will fix security holes for N years."
Nobody can hold the maker to it. The buyer has no remedy when a known vulnerability sits
unpatched for months, and a second-hand buyer does not even know what was promised.
Regulation (the EU Cyber Resilience Act) now makes security updates a legal duty, but
enforcement is a regulator's job and takes years. The person holding the vulnerable
device gets nothing.

## What Patchbond does

The maker puts money behind the promise, per unit, and the unit's NFT holds it.

1. **Mint.** The maker opens a product line (patch window, arbiter, advisory stake) and
   mints one NEP-171 passport per unit sold. The NEAR attached to the mint is locked
   inside that passport as its bond. The passport also commits to the SHA-256 of the
   unit's SBOM, the firmware version and the end-of-support date.
2. **Advisory.** Anyone can file a security advisory against the line by staking NEAR.
   That starts the patch clock for every unit of that line under support.
3. **Patch or pay.**
   - Maker publishes the fix inside the window: nothing is owed, reporter gets the stake back.
   - Deadline missed: the **current owner of the NFT** collects 10% / 25% / 50% of the
     unit's original bond (low / high / critical), capped by what is left.
     A late patch is recorded but does not cancel the claim.
4. **Dispute.** If the advisory is bogus, the maker can dispute it once, paying a fee equal
   to the reporter's stake. The clock pauses. The line's arbiter rules: upheld → fee goes
   to the reporter and the clock restarts with a full window; rejected → reporter's stake
   goes to the maker.
5. **Resale.** `nft_transfer` moves the passport. The bond and the right to collect move
   with it; the seller can no longer claim.
6. **End of support.** The maker calls `release`. Anything the owner was still owed is paid
   to the owner first, the rest of the bond returns to the maker. The passport stays with
   the owner as the unit's permanent record.

## Why NEAR

- **The NFT is the claim.** The right to collect is "whoever `nft_token` says owns this id
  right now", so a standard NEP-171 transfer is all a resale needs. No registry to update.
- **Live status in a standard field.** `nft_token` injects the current status and remaining
  bond into `metadata.extra`, so any NEP-171 wallet or marketplace can show whether the
  device is covered without knowing about Patchbond.
- **Named accounts** make the parties legible: a buyer sees `acme-cam.testnet` as maker and
  `pb-cert.testnet` as arbiter, not two hex strings.
- **Storage staking** makes the per-unit cost explicit: the contract measures the bytes each
  mint adds, pays for them out of the attached deposit, and bonds the rest (about 0.01 NEAR
  of storage per unit).
- **Sub-cent fees and ~1s finality** make a per-unit bond practical for a 40-dollar camera.

## Try it

### 1. Look, no setup (30 seconds)

Open https://aditya-galaxy.github.io/patchbond/ . It reads the testnet contract directly
over RPC. Three seeded passports show three different states:

| Open this | You see |
| --- | --- |
| [`CAM-S2-0001`](https://aditya-galaxy.github.io/patchbond/?token=CAM-S2-0001) | Status **BREACH CLAIMABLE**. A critical advisory missed its deadline; the page shows the owner can collect 0.995 NEAR and the exact command. |
| [`CAM-S2-0002`](https://aditya-galaxy.github.io/patchbond/?token=CAM-S2-0002) | Same advisory, but this owner already collected: bond bar at 50%, advisory marked **OWNER PAID**. |
| [`BELL-0001`](https://aditya-galaxy.github.io/patchbond/?token=BELL-0001) | Status **ADVISORY OPEN** on a line with a 7-day patch window; a live countdown to the maker's deadline. |

Or query the contract yourself:

```sh
curl -s https://test.rpc.fastnear.com -H 'Content-Type: application/json' -d '{"jsonrpc":"2.0","id":1,"method":"query","params":{"request_type":"call_function","finality":"final","account_id":"patchbond.testnet","method_name":"get_unit","args_base64":"eyJ0b2tlbl9pZCI6IkNBTS1TMi0wMDAxIn0="}}' \
  | python3 -c 'import sys,json; print(bytes(json.load(sys.stdin)["result"]["result"]).decode())'
```

### 2. Run the tests (2 minutes)

Needs Rust 1.95 or newer (the `near-sdk` 5.30 minimum).

```sh
cd contract && cargo test
```

You see `test result: ok. 26 passed`. The tests cover the money paths: bond = deposit minus
storage, payout by severity, payouts never exceed the bond, no double claim, claim follows
the NFT, seller loses the claim, late patch still pays, dispute upheld / rejected, release
blocked while an advisory is pending, release settles the owner first.

### 3. Run the whole lifecycle on testnet (about 4 minutes)

Needs [`near-cli-rs`](https://github.com/near/near-cli-rs), `curl`, `jq`, `python3`.
You do not need to deploy anything: `patchbond.testnet` is public and anyone may open a
product line on it.

```sh
scripts/setup-accounts.sh yourprefix   # creates 5 free faucet accounts, prints the next command
MAKER=yourprefix-maker.testnet ALICE=yourprefix-alice.testnet BOB=yourprefix-bob.testnet \
REPORTER=yourprefix-reporter.testnet ARBITER=yourprefix-cert.testnet scripts/demo.sh
```

The script prints a transaction link for every step. What you see, in order:

| Step | Do | See |
| --- | --- | --- |
| 1 | Maker opens a line with a 40s patch window | `create_line` succeeds |
| 2 | Maker mints a unit for Alice with 2 NEAR attached | `status covered … bond 1.99… NEAR` (2 NEAR minus about 0.01 NEAR of storage) |
| 3 | Advisory A filed, maker patches in time | `#N CVE-2022-0778 [High] → patched`, bond untouched |
| 4 | Alice transfers the NFT to Bob | owner becomes Bob |
| 5 | Advisory B (critical) filed, maker does nothing | Bob's early claim is **refused**; after 40s status is `breach_claimable`; Alice's claim is **refused**; Bob's claim pays and his balance rises by about 0.995 NEAR; a second claim is **refused** |
| 6 | Advisory C is bogus; maker disputes, arbiter rejects | `→ rejected`, bond untouched |
| 7 | Support period ends, maker releases | early release is **refused**; then maker's balance rises by the remaining 0.995 NEAR; status `released` |

Open the passport URL the script prints at the start to watch the same unit change state live.

Run it against your own deployment with `CONTRACT=your.testnet scripts/demo.sh` after:

```sh
cd contract && cargo near build non-reproducible-wasm
near contract deploy your.testnet use-file target/near/patchbond.wasm with-init-call new \
  json-args '{"owner_id":"your.testnet"}' prepaid-gas '30 Tgas' attached-deposit '0 NEAR' \
  network-config testnet sign-with-legacy-keychain send
```

### 4. Check an SBOM against a passport

```sh
scripts/verify-sbom.sh CAM-S2-0001 sbom/acme-cam-s2-1.4.0.cdx.json
```

Prints `MATCH` when the file hashes to what the maker locked into the passport.

## Contract interface

`contract/src/lib.rs`, one file. All timestamps are unix seconds; amounts are yoctoNEAR strings.

| Who | Method | Notes |
| --- | --- | --- |
| maker | `create_line(line_id, name, arbiter, patch_window_sec, advisory_stake)` | caller becomes the maker; arbiter must be a different account |
| maker | `mint_unit(token_id, line_id, owner_id, sbom_sha256, firmware, support_until)` payable | deposit minus storage becomes the bond; returns the bond |
| maker | `publish_patch(advisory_id, fix_firmware, fix_sha256)` | in time → `Patched`; late → `PatchedLate` (still claimable) |
| maker | `dispute(advisory_id)` payable | before the deadline, once, fee ≥ advisory stake |
| maker | `release(token_id)` | after `support_until`; settles the owner, then returns the rest |
| anyone | `file_advisory(line_id, reference, component, severity)` payable | stake ≥ the line's `advisory_stake`; returns the advisory id |
| reporter | `reclaim_stake(advisory_id)` | after a missed deadline |
| arbiter | `resolve_dispute(advisory_id, upheld, note)` | |
| owner | `claim(token_id, advisory_id)` | current NFT owner only |
| view | `get_unit(token_id)` | status, bond, remaining, per-advisory state and claimable amount |
| view | `get_line`, `get_advisory`, `get_line_advisories`, `advisory_count` | |
| NEP-171/177/181 | `nft_transfer`, `nft_transfer_call`, `nft_token`, `nft_tokens`, `nft_tokens_for_owner`, `nft_metadata`, … | `nft_token` carries live status in `metadata.extra` |

Unit status: `covered` · `advisory_open` · `breach_claimable` · `end_of_support` · `released`.

An advisory covers a unit if it was filed against the unit's line while the unit was
under support (after mint, before `support_until`).

## Limits, stated plainly

- **Testnet only.** Nothing is deployed to mainnet. Amounts in the demo are faucet NEAR.
- **The arbiter is a trusted account** chosen by the maker when the line is opened and shown
  on every passport. It only matters when the maker disputes; an undisputed advisory needs
  no one's judgement. Pointing it at a Sputnik DAO is the intended production setup; that is
  not built here.
- **Advisories are line-wide.** The contract does not check on-chain that the named component
  is in a unit's SBOM (only the SBOM hash is on-chain). A wrong advisory is what the dispute
  path is for.
- **The bond is accounted per token inside one contract**, not held in a separate account per NFT.
- **Bond is in NEAR**, so its fiat value moves. A stablecoin (NEP-141) bond is the obvious next step.
- **The page is read-only.** Writes go through `near-cli-rs`; there is no wallet connection.
- **At most 200 advisories per line**, so `release` and `get_unit` stay within gas.
- **Not audited.**

## Layout

```
contract/src/lib.rs    the contract (NEP-171 passport + bond, advisory, dispute, release logic)
contract/src/tests.rs  26 unit tests
docs/index.html        the passport page (single static file, no build step, served by GitHub Pages)
scripts/demo.sh        full lifecycle on testnet
scripts/seed.sh        seeds the showcase passports the page opens by default
scripts/setup-accounts.sh, verify-sbom.sh, lib.sh
video/                 how the demo video was recorded (real run + headless Chrome screenshots + ffmpeg)
sbom/                  sample CycloneDX SBOM whose hash is locked into the demo passports
```

## Disclosures

- All code in this repository was written during the hackathon window (first commit 14 Nov 2026). There is no pre-hackathon baseline.
- Written with an AI coding agent (Claude Code); reviewed and run by the author.
- Libraries: [`near-sdk`](https://crates.io/crates/near-sdk) 5.30 and [`near-contract-standards`](https://crates.io/crates/near-contract-standards) 5.30 (NEP-171 implementation). Tooling: `cargo-near`, `near-cli-rs`. The page uses no libraries.
- The CVE ids used in the demo are real identifiers for the named open-source components, used as sample data. "Acme" is a made-up maker.

## License

MIT
