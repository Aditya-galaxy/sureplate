//! Sureplate: a device passport NFT that escrows the maker's security-update bond.
//!
//! A maker opens a product line, then mints one NFT per unit sold and locks NEAR
//! inside it. If a security advisory against the line is not patched inside the
//! line's patch window, the *current* NFT owner collects a slice of that bond.
//! When the support period ends with nothing outstanding, the maker gets the rest back.
//!
//! All timestamps in the public API are unix seconds.

use std::collections::HashMap;

use near_contract_standards::non_fungible_token::core::{
    NonFungibleTokenCore, NonFungibleTokenResolver,
};
use near_contract_standards::non_fungible_token::enumeration::NonFungibleTokenEnumeration;
use near_contract_standards::non_fungible_token::metadata::{
    NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
};
use near_contract_standards::non_fungible_token::{NonFungibleToken, Token, TokenId};
use near_sdk::borsh::BorshSerialize;
use near_sdk::json_types::U128;
use near_sdk::serde_json::json;
use near_sdk::store::{LookupMap, LookupSet, Vector};
use near_sdk::{
    env, near, require, AccountId, BorshStorageKey, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};

/// Hard cap on advisories per product line, so `release` and views stay bounded.
pub const MAX_ADVISORIES_PER_LINE: usize = 200;
/// Shortest patch window a line may promise (seconds).
pub const MIN_PATCH_WINDOW_SEC: u64 = 30;
/// Smallest bond a unit may carry.
pub const MIN_BOND: NearToken = NearToken::from_millinear(10);

#[derive(BorshSerialize, BorshStorageKey)]
#[borsh(crate = "near_sdk::borsh")]
enum Key {
    OwnerById,
    TokenMetadata,
    Enumeration,
    Lines,
    Units,
    Advisories,
    Claims,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Severity {
    Low,
    High,
    Critical,
}

impl Severity {
    /// Share of the unit's original bond owed to the owner on a breach, in basis points.
    pub fn payout_bps(&self) -> u128 {
        match self {
            Severity::Low => 1_000,
            Severity::High => 2_500,
            Severity::Critical => 5_000,
        }
    }
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AdvisoryStatus {
    /// Clock is running (or has run out: see `deadline`).
    Open,
    /// Maker disputed in time; clock paused until the arbiter rules.
    Disputed,
    /// Maker published a fix inside the window.
    Patched,
    /// Maker published a fix after the window. Owners can still claim.
    PatchedLate,
    /// Arbiter ruled the advisory invalid.
    Rejected,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug)]
pub struct ProductLine {
    pub maker: AccountId,
    /// Account that rules on disputed advisories (a CERT, an auditor, a DAO).
    pub arbiter: AccountId,
    pub name: String,
    pub patch_window_sec: u64,
    /// Stake a reporter must attach to file an advisory against this line.
    pub advisory_stake: NearToken,
    pub advisory_ids: Vec<u64>,
    pub units: u64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug)]
pub struct Unit {
    pub line_id: String,
    pub bond: NearToken,
    pub remaining: NearToken,
    /// SHA-256 of the unit's SBOM (CycloneDX/SPDX file), hex.
    pub sbom_sha256: String,
    pub firmware: String,
    pub minted_at: u64,
    pub support_until: u64,
    pub released: bool,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug)]
pub struct Advisory {
    pub id: u64,
    pub line_id: String,
    pub reporter: AccountId,
    /// CVE / GHSA / OSV id or URL.
    pub reference: String,
    /// Affected component, ideally a purl from the SBOM.
    pub component: String,
    pub severity: Severity,
    pub filed_at: u64,
    pub deadline: u64,
    pub status: AdvisoryStatus,
    /// Reporter stake still held by the contract.
    pub stake: NearToken,
    /// Maker's dispute fee still held by the contract.
    pub dispute_fee: NearToken,
    pub disputed: bool,
    pub fix_firmware: Option<String>,
    pub fix_sha256: Option<String>,
    pub resolved_at: Option<u64>,
    pub note: Option<String>,
}

impl Advisory {
    pub fn breached(&self, now: u64) -> bool {
        match self.status {
            AdvisoryStatus::Open => now > self.deadline,
            AdvisoryStatus::PatchedLate => true,
            _ => false,
        }
    }
    pub fn pending(&self, now: u64) -> bool {
        match self.status {
            AdvisoryStatus::Open => now <= self.deadline,
            AdvisoryStatus::Disputed => true,
            _ => false,
        }
    }
    /// An advisory covers a unit if it was filed while that unit was under support.
    pub fn applies_to(&self, unit: &Unit) -> bool {
        self.line_id == unit.line_id
            && unit.minted_at <= self.filed_at
            && self.filed_at <= unit.support_until
    }
}

#[near(serializers = [json])]
pub struct UnitAdvisoryView {
    pub advisory: Advisory,
    /// pending | disputed | patched | rejected | breach_claimable | breach_claimed
    pub state: String,
    /// What `claim` would pay the owner right now for this advisory.
    pub claimable: NearToken,
}

#[near(serializers = [json])]
pub struct UnitView {
    pub token_id: TokenId,
    pub owner_id: AccountId,
    pub line_id: String,
    pub line_name: String,
    pub maker: AccountId,
    pub arbiter: AccountId,
    pub patch_window_sec: u64,
    pub bond: NearToken,
    pub remaining: NearToken,
    pub sbom_sha256: String,
    pub firmware: String,
    pub minted_at: u64,
    pub support_until: u64,
    /// covered | advisory_open | breach_claimable | end_of_support | released
    pub status: String,
    pub advisories: Vec<UnitAdvisoryView>,
    pub now: u64,
}

#[near(serializers = [json])]
pub struct ReleaseOutcome {
    pub paid_to_owner: NearToken,
    pub returned_to_maker: NearToken,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    tokens: NonFungibleToken,
    metadata: NFTContractMetadata,
    lines: LookupMap<String, ProductLine>,
    units: LookupMap<TokenId, Unit>,
    advisories: Vector<Advisory>,
    /// "<token_id>#<advisory_id>" for every breach already paid out.
    claims: LookupSet<String>,
}

fn now_sec() -> u64 {
    env::block_timestamp() / 1_000_000_000
}

fn emit(event: &str, data: near_sdk::serde_json::Value) {
    env::log_str(&format!(
        "EVENT_JSON:{}",
        json!({"standard": "sureplate", "version": "1.0.0", "event": event, "data": [data]})
    ));
}

fn pay(to: &AccountId, amount: NearToken) {
    if !amount.is_zero() {
        Promise::new(to.clone()).transfer(amount).detach();
    }
}

fn claim_key(token_id: &str, advisory_id: u64) -> String {
    format!("{token_id}#{advisory_id}")
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner_id: AccountId) -> Self {
        Self {
            tokens: NonFungibleToken::new(
                Key::OwnerById,
                owner_id,
                Some(Key::TokenMetadata),
                Some(Key::Enumeration),
                None::<Key>,
            ),
            metadata: NFTContractMetadata {
                spec: NFT_METADATA_SPEC.to_string(),
                name: "Sureplate device passports".to_string(),
                symbol: "SPLATE".to_string(),
                icon: None,
                base_uri: None,
                reference: None,
                reference_hash: None,
            },
            lines: LookupMap::new(Key::Lines),
            units: LookupMap::new(Key::Units),
            advisories: Vector::new(Key::Advisories),
            claims: LookupSet::new(Key::Claims),
        }
    }

    // ───────────────────────── maker ─────────────────────────

    /// Open a product line. The caller becomes its maker.
    pub fn create_line(
        &mut self,
        line_id: String,
        name: String,
        arbiter: AccountId,
        patch_window_sec: u64,
        advisory_stake: NearToken,
    ) {
        require!(
            !line_id.is_empty() && line_id.len() <= 64 && !line_id.contains('#'),
            "line_id: 1-64 chars, no '#'"
        );
        require!(!name.is_empty() && name.len() <= 120, "name: 1-120 chars");
        require!(
            patch_window_sec >= MIN_PATCH_WINDOW_SEC,
            "patch window too short"
        );
        require!(!self.lines.contains_key(&line_id), "line already exists");
        let maker = env::predecessor_account_id();
        require!(arbiter != maker, "arbiter must not be the maker");
        emit(
            "line_created",
            json!({"line_id": line_id, "maker": maker, "arbiter": arbiter, "patch_window_sec": patch_window_sec}),
        );
        self.lines.insert(
            line_id,
            ProductLine {
                maker,
                arbiter,
                name,
                patch_window_sec,
                advisory_stake,
                advisory_ids: vec![],
                units: 0,
            },
        );
    }

    /// Mint the passport for one unit and lock the attached NEAR inside it as its bond.
    /// Storage for the new records is paid out of the attached deposit; the rest is the bond.
    #[payable]
    pub fn mint_unit(
        &mut self,
        token_id: TokenId,
        line_id: String,
        owner_id: AccountId,
        sbom_sha256: String,
        firmware: String,
        support_until: u64,
    ) -> NearToken {
        let storage_before = env::storage_usage();
        let now = now_sec();
        let line = self.lines.get_mut(&line_id).expect("unknown line");
        require!(
            env::predecessor_account_id() == line.maker,
            "only the line's maker can mint"
        );
        require!(
            !token_id.is_empty() && token_id.len() <= 64 && !token_id.contains('#'),
            "token_id: 1-64 chars, no '#'"
        );
        require!(is_sha256_hex(&sbom_sha256), "sbom_sha256 must be 64 hex chars");
        require!(!firmware.is_empty() && firmware.len() <= 64, "firmware: 1-64 chars");
        require!(support_until > now, "support_until must be in the future");
        line.units += 1;
        let title = format!("{} · {}", line.name, token_id);

        self.tokens.internal_mint_with_refund(
            token_id.clone(),
            owner_id.clone(),
            Some(TokenMetadata {
                title: Some(title),
                description: Some(
                    "Sureplate device passport. Holds the maker's security-update bond for this unit."
                        .to_string(),
                ),
                media: None,
                media_hash: None,
                copies: Some(1),
                issued_at: Some((now * 1000).to_string()),
                expires_at: Some((support_until * 1000).to_string()),
                starts_at: None,
                updated_at: None,
                extra: None,
                reference: None,
                reference_hash: None,
            }),
            None,
        );
        self.units.insert(
            token_id.clone(),
            Unit {
                line_id: line_id.clone(),
                bond: NearToken::from_yoctonear(0),
                remaining: NearToken::from_yoctonear(0),
                sbom_sha256: sbom_sha256.to_lowercase(),
                firmware,
                minted_at: now,
                support_until,
                released: false,
            },
        );
        self.units.flush();
        self.lines.flush();

        let storage_cost = env::storage_byte_cost()
            .saturating_mul((env::storage_usage().saturating_sub(storage_before)) as u128);
        let bond = env::attached_deposit()
            .checked_sub(storage_cost)
            .expect("deposit does not cover storage");
        require!(bond >= MIN_BOND, "bond below minimum (0.01 NEAR after storage)");
        let unit = self.units.get_mut(&token_id).unwrap();
        unit.bond = bond;
        unit.remaining = bond;
        emit(
            "unit_minted",
            json!({"token_id": token_id, "line_id": line_id, "owner_id": owner_id, "bond": bond, "support_until": support_until}),
        );
        bond
    }

    /// Publish the fix for an advisory. In time: the advisory closes clean.
    /// Late: recorded, but owners keep their claim.
    pub fn publish_patch(&mut self, advisory_id: u64, fix_firmware: String, fix_sha256: String) {
        let now = now_sec();
        let adv = self.advisories.get_mut(advisory_id as u32).expect("unknown advisory");
        let line = self.lines.get(&adv.line_id).unwrap();
        require!(env::predecessor_account_id() == line.maker, "only the maker can patch");
        require!(adv.status == AdvisoryStatus::Open, "advisory is not open");
        require!(is_sha256_hex(&fix_sha256), "fix_sha256 must be 64 hex chars");
        require!(!fix_firmware.is_empty() && fix_firmware.len() <= 64, "fix_firmware: 1-64 chars");
        adv.status = if now <= adv.deadline {
            AdvisoryStatus::Patched
        } else {
            AdvisoryStatus::PatchedLate
        };
        adv.fix_firmware = Some(fix_firmware);
        adv.fix_sha256 = Some(fix_sha256.to_lowercase());
        adv.resolved_at = Some(now);
        let stake = adv.stake;
        adv.stake = NearToken::from_yoctonear(0);
        pay(&adv.reporter, stake);
        emit(
            "patch_published",
            json!({"advisory_id": advisory_id, "in_time": adv.status == AdvisoryStatus::Patched}),
        );
    }

    /// Dispute an advisory before its deadline. Pauses the clock until the arbiter rules.
    /// Costs the maker a fee equal to the reporter's stake; it goes to the reporter if the
    /// advisory is upheld. One dispute per advisory.
    #[payable]
    pub fn dispute(&mut self, advisory_id: u64) {
        let now = now_sec();
        let adv = self.advisories.get_mut(advisory_id as u32).expect("unknown advisory");
        let line = self.lines.get(&adv.line_id).unwrap();
        require!(env::predecessor_account_id() == line.maker, "only the maker can dispute");
        require!(adv.status == AdvisoryStatus::Open && now <= adv.deadline, "too late to dispute");
        require!(!adv.disputed, "already disputed once");
        require!(
            env::attached_deposit() >= line.advisory_stake,
            "attach a dispute fee equal to the advisory stake"
        );
        adv.status = AdvisoryStatus::Disputed;
        adv.disputed = true;
        adv.dispute_fee = env::attached_deposit();
        emit("advisory_disputed", json!({"advisory_id": advisory_id}));
    }

    /// After support ends: settle anything the owner is still owed, then return the rest of
    /// the bond to the maker. Fails while any advisory covering this unit is still pending.
    pub fn release(&mut self, token_id: TokenId) -> ReleaseOutcome {
        let now = now_sec();
        let unit = self.units.get(&token_id).expect("unknown unit").clone();
        let line = self.lines.get(&unit.line_id).unwrap().clone();
        require!(env::predecessor_account_id() == line.maker, "only the maker can release");
        require!(!unit.released, "already released");
        require!(now > unit.support_until, "support period has not ended");

        let mut remaining = unit.remaining;
        let mut owed = NearToken::from_yoctonear(0);
        for id in line.advisory_ids.iter() {
            let adv = self.advisories.get(*id as u32).unwrap();
            if !adv.applies_to(&unit) {
                continue;
            }
            require!(!adv.pending(now), "an advisory covering this unit is still pending");
            let key = claim_key(&token_id, *id);
            if adv.breached(now) && !self.claims.contains(&key) {
                let amount = payout(&unit, adv.severity, remaining);
                remaining = remaining.saturating_sub(amount);
                owed = owed.saturating_add(amount);
                self.claims.insert(key);
            }
        }
        let owner = self.owner_of(&token_id);
        let unit = self.units.get_mut(&token_id).unwrap();
        unit.remaining = NearToken::from_yoctonear(0);
        unit.released = true;
        pay(&owner, owed);
        pay(&line.maker, remaining);
        emit(
            "bond_released",
            json!({"token_id": token_id, "paid_to_owner": owed, "returned_to_maker": remaining}),
        );
        ReleaseOutcome { paid_to_owner: owed, returned_to_maker: remaining }
    }

    // ───────────────────────── reporter ─────────────────────────

    /// File a security advisory against a product line. Starts the patch clock for every
    /// unit of that line currently under support. Requires the line's advisory stake.
    #[payable]
    pub fn file_advisory(
        &mut self,
        line_id: String,
        reference: String,
        component: String,
        severity: Severity,
    ) -> u64 {
        let now = now_sec();
        let line = self.lines.get_mut(&line_id).expect("unknown line");
        require!(
            env::attached_deposit() >= line.advisory_stake,
            "attach the line's advisory stake"
        );
        require!(!reference.is_empty() && reference.len() <= 200, "reference: 1-200 chars");
        require!(component.len() <= 200, "component: max 200 chars");
        require!(
            line.advisory_ids.len() < MAX_ADVISORIES_PER_LINE,
            "advisory limit reached for this line"
        );
        let id = self.advisories.len() as u64;
        let deadline = now + line.patch_window_sec;
        line.advisory_ids.push(id);
        let reporter = env::predecessor_account_id();
        emit(
            "advisory_filed",
            json!({"advisory_id": id, "line_id": line_id, "reference": reference, "severity": severity, "deadline": deadline, "reporter": reporter}),
        );
        self.advisories.push(Advisory {
            id,
            line_id,
            reporter,
            reference,
            component,
            severity,
            filed_at: now,
            deadline,
            status: AdvisoryStatus::Open,
            stake: env::attached_deposit(),
            dispute_fee: NearToken::from_yoctonear(0),
            disputed: false,
            fix_firmware: None,
            fix_sha256: None,
            resolved_at: None,
            note: None,
        });
        id
    }

    /// Reporter takes the stake back once the maker has blown the deadline.
    pub fn reclaim_stake(&mut self, advisory_id: u64) -> NearToken {
        let now = now_sec();
        let adv = self.advisories.get_mut(advisory_id as u32).expect("unknown advisory");
        require!(env::predecessor_account_id() == adv.reporter, "only the reporter");
        require!(
            adv.status == AdvisoryStatus::Open && now > adv.deadline,
            "stake is returned on patch, or reclaimable after a missed deadline"
        );
        let stake = adv.stake;
        require!(!stake.is_zero(), "nothing to reclaim");
        adv.stake = NearToken::from_yoctonear(0);
        pay(&adv.reporter, stake);
        stake
    }

    // ───────────────────────── arbiter ─────────────────────────

    /// Rule on a disputed advisory.
    /// Upheld: the clock restarts with a full window and the maker's fee goes to the reporter.
    /// Rejected: the advisory is void and the reporter's stake goes to the maker.
    pub fn resolve_dispute(&mut self, advisory_id: u64, upheld: bool, note: Option<String>) {
        let now = now_sec();
        let adv = self.advisories.get_mut(advisory_id as u32).expect("unknown advisory");
        let line = self.lines.get(&adv.line_id).unwrap();
        require!(env::predecessor_account_id() == line.arbiter, "only the line's arbiter");
        require!(adv.status == AdvisoryStatus::Disputed, "advisory is not disputed");
        require!(note.as_ref().is_none_or(|n| n.len() <= 280), "note: max 280 chars");
        let fee = adv.dispute_fee;
        adv.dispute_fee = NearToken::from_yoctonear(0);
        adv.note = note;
        if upheld {
            adv.status = AdvisoryStatus::Open;
            adv.deadline = now + line.patch_window_sec;
            pay(&adv.reporter, fee);
        } else {
            adv.status = AdvisoryStatus::Rejected;
            adv.resolved_at = Some(now);
            let stake = adv.stake;
            adv.stake = NearToken::from_yoctonear(0);
            pay(&line.maker, stake.saturating_add(fee));
        }
        emit("dispute_resolved", json!({"advisory_id": advisory_id, "upheld": upheld}));
    }

    // ───────────────────────── owner ─────────────────────────

    /// Collect on a missed patch deadline. Only the current NFT owner can call this, so the
    /// right to collect travels with the device when the NFT is transferred.
    pub fn claim(&mut self, token_id: TokenId, advisory_id: u64) -> NearToken {
        let now = now_sec();
        let owner = self.owner_of(&token_id);
        require!(env::predecessor_account_id() == owner, "only the current owner can claim");
        let adv = self.advisories.get(advisory_id as u32).expect("unknown advisory");
        let unit = self.units.get(&token_id).expect("unknown unit");
        require!(!unit.released, "bond already released");
        require!(adv.applies_to(unit), "advisory does not cover this unit");
        require!(adv.breached(now), "patch deadline has not been missed");
        let key = claim_key(&token_id, advisory_id);
        require!(!self.claims.contains(&key), "already claimed");
        let amount = payout(unit, adv.severity, unit.remaining);
        require!(!amount.is_zero(), "bond is exhausted");
        self.claims.insert(key);
        let unit = self.units.get_mut(&token_id).unwrap();
        unit.remaining = unit.remaining.saturating_sub(amount);
        pay(&owner, amount);
        emit(
            "breach_claimed",
            json!({"token_id": token_id, "advisory_id": advisory_id, "owner_id": owner, "amount": amount}),
        );
        amount
    }

    // ───────────────────────── views ─────────────────────────

    pub fn get_line(&self, line_id: String) -> Option<ProductLine> {
        self.lines.get(&line_id).cloned()
    }

    pub fn get_advisory(&self, advisory_id: u64) -> Option<Advisory> {
        self.advisories.get(advisory_id as u32).cloned()
    }

    pub fn get_line_advisories(&self, line_id: String) -> Vec<Advisory> {
        match self.lines.get(&line_id) {
            Some(line) => line
                .advisory_ids
                .iter()
                .map(|id| self.advisories.get(*id as u32).unwrap().clone())
                .collect(),
            None => vec![],
        }
    }

    pub fn advisory_count(&self) -> u64 {
        self.advisories.len() as u64
    }

    /// Everything a buyer needs to know about one unit, computed at the current block time.
    pub fn get_unit(&self, token_id: TokenId) -> Option<UnitView> {
        let now = now_sec();
        let unit = self.units.get(&token_id)?;
        let line = self.lines.get(&unit.line_id).unwrap();
        let mut remaining = unit.remaining;
        let mut any_pending = false;
        let mut any_claimable = false;
        let mut advisories = vec![];
        for id in line.advisory_ids.iter() {
            let adv = self.advisories.get(*id as u32).unwrap();
            if !adv.applies_to(unit) {
                continue;
            }
            let mut claimable = NearToken::from_yoctonear(0);
            let state = if adv.breached(now) {
                if self.claims.contains(&claim_key(&token_id, *id)) {
                    "breach_claimed"
                } else if unit.released {
                    "breach_claimed"
                } else {
                    claimable = payout(unit, adv.severity, remaining);
                    remaining = remaining.saturating_sub(claimable);
                    any_claimable = any_claimable || !claimable.is_zero();
                    "breach_claimable"
                }
            } else {
                match adv.status {
                    AdvisoryStatus::Disputed => {
                        any_pending = true;
                        "disputed"
                    }
                    AdvisoryStatus::Open => {
                        any_pending = true;
                        "pending"
                    }
                    AdvisoryStatus::Patched => "patched",
                    _ => "rejected",
                }
            };
            advisories.push(UnitAdvisoryView {
                advisory: adv.clone(),
                state: state.to_string(),
                claimable,
            });
        }
        let status = if unit.released {
            "released"
        } else if any_claimable {
            "breach_claimable"
        } else if any_pending {
            "advisory_open"
        } else if now > unit.support_until {
            "end_of_support"
        } else {
            "covered"
        };
        Some(UnitView {
            token_id: token_id.clone(),
            owner_id: self.owner_of(&token_id),
            line_id: unit.line_id.clone(),
            line_name: line.name.clone(),
            maker: line.maker.clone(),
            arbiter: line.arbiter.clone(),
            patch_window_sec: line.patch_window_sec,
            bond: unit.bond,
            remaining: unit.remaining,
            sbom_sha256: unit.sbom_sha256.clone(),
            firmware: unit.firmware.clone(),
            minted_at: unit.minted_at,
            support_until: unit.support_until,
            status: status.to_string(),
            advisories,
            now,
        })
    }
}

impl Contract {
    fn owner_of(&self, token_id: &TokenId) -> AccountId {
        self.tokens.owner_by_id.get(token_id).expect("unknown unit")
    }
}

/// Severity share of the original bond, capped by what is left.
fn payout(unit: &Unit, severity: Severity, remaining: NearToken) -> NearToken {
    let share = unit.bond.as_yoctonear() * severity.payout_bps() / 10_000;
    NearToken::from_yoctonear(share.min(remaining.as_yoctonear()))
}

// ───────────────────────── NEP-171 / NEP-177 / NEP-181 ─────────────────────────

#[near]
impl NonFungibleTokenCore for Contract {
    #[payable]
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        self.tokens.nft_transfer(receiver_id, token_id, approval_id, memo)
    }

    #[payable]
    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        self.tokens.nft_transfer_call(receiver_id, token_id, approval_id, memo, msg)
    }

    /// Standard token view, with the live bond state injected into `metadata.extra`
    /// so any NEP-171 wallet or marketplace can show it.
    fn nft_token(&self, token_id: TokenId) -> Option<Token> {
        let mut token = self.tokens.nft_token(token_id.clone())?;
        if let (Some(meta), Some(view)) = (token.metadata.as_mut(), self.get_unit(token_id)) {
            meta.extra = Some(
                json!({
                    "status": view.status,
                    "bond": view.bond,
                    "remaining": view.remaining,
                    "line_id": view.line_id,
                    "maker": view.maker,
                    "sbom_sha256": view.sbom_sha256,
                    "firmware": view.firmware,
                    "support_until": view.support_until,
                })
                .to_string(),
            );
        }
        Some(token)
    }
}

#[near]
impl NonFungibleTokenResolver for Contract {
    #[private]
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
    ) -> bool {
        self.tokens.nft_resolve_transfer(
            previous_owner_id,
            receiver_id,
            token_id,
            approved_account_ids,
        )
    }
}

#[near]
impl NonFungibleTokenEnumeration for Contract {
    fn nft_total_supply(&self) -> U128 {
        self.tokens.nft_total_supply()
    }
    fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        self.tokens.nft_tokens(from_index, limit)
    }
    fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        self.tokens.nft_supply_for_owner(account_id)
    }
    fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        self.tokens.nft_tokens_for_owner(account_id, from_index, limit)
    }
}

#[near]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata.clone()
    }
}

#[cfg(test)]
mod tests;
