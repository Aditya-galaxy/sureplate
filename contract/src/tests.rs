use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

const SBOM: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
const FIX: &str = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";
const T0: u64 = 1_800_000_000;
const WINDOW: u64 = 3_600;
const SUPPORT: u64 = T0 + 1_000_000;

fn acc(s: &str) -> AccountId {
    s.parse().unwrap()
}
fn near(n: u128) -> NearToken {
    NearToken::from_near(n)
}

/// Switch caller, clock (unix seconds) and attached deposit.
fn at(who: &str, t: u64, deposit: NearToken) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(acc("patchbond.testnet"))
        .predecessor_account_id(acc(who))
        .signer_account_id(acc(who))
        .block_timestamp(t * 1_000_000_000)
        .attached_deposit(deposit)
        .account_balance(near(1_000))
        .build());
}
fn zero() -> NearToken {
    NearToken::from_yoctonear(0)
}

/// A line by `maker`, one unit `cam-1` owned by `alice` with a ~10 NEAR bond.
fn setup() -> (Contract, NearToken) {
    at("maker", T0, zero());
    let mut c = Contract::new(acc("patchbond.testnet"));
    c.create_line("cam".into(), "Acme Cam".into(), acc("cert"), WINDOW, near(1));
    at("maker", T0, near(10));
    let bond = c.mint_unit("cam-1".into(), "cam".into(), acc("alice"), SBOM.into(), "1.0.0".into(), SUPPORT);
    (c, bond)
}
fn file(c: &mut Contract, t: u64, sev: Severity) -> u64 {
    at("reporter", t, near(1));
    c.file_advisory("cam".into(), "CVE-2026-0001".into(), "pkg:generic/openssl@3.0.1".into(), sev)
}

#[test]
fn mint_locks_deposit_minus_storage() {
    let (c, bond) = setup();
    assert!(bond < near(10) && bond > NearToken::from_millinear(9_900), "bond = {bond}");
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.status, "covered");
    assert_eq!(u.owner_id, acc("alice"));
    assert_eq!(u.remaining, bond);
    assert_eq!(c.nft_total_supply().0, 1);
    assert_eq!(c.get_line("cam".into()).unwrap().units, 1);
}

#[test]
#[should_panic(expected = "only the line's maker can mint")]
fn stranger_cannot_mint() {
    let (mut c, _) = setup();
    at("mallory", T0, near(10));
    c.mint_unit("cam-2".into(), "cam".into(), acc("mallory"), SBOM.into(), "1.0.0".into(), SUPPORT);
}

#[test]
#[should_panic(expected = "sbom_sha256 must be 64 hex chars")]
fn mint_needs_real_hash() {
    let (mut c, _) = setup();
    at("maker", T0, near(10));
    c.mint_unit("cam-2".into(), "cam".into(), acc("alice"), "nope".into(), "1.0.0".into(), SUPPORT);
}

#[test]
#[should_panic(expected = "attach the line's advisory stake")]
fn advisory_needs_stake() {
    let (mut c, _) = setup();
    at("reporter", T0 + 10, NearToken::from_millinear(1));
    c.file_advisory("cam".into(), "CVE-1".into(), "".into(), Severity::Low);
}

#[test]
fn patched_in_time_pays_nobody() {
    let (mut c, bond) = setup();
    let id = file(&mut c, T0 + 10, Severity::Critical);
    assert_eq!(c.get_unit("cam-1".into()).unwrap().status, "advisory_open");

    at("maker", T0 + 10 + WINDOW, zero()); // exactly on the deadline still counts
    c.publish_patch(id, "1.0.1".into(), FIX.into());
    let adv = c.get_advisory(id).unwrap();
    assert_eq!(adv.status, AdvisoryStatus::Patched);
    assert!(adv.stake.is_zero(), "stake returned to reporter");

    at("alice", T0 + 10 + WINDOW + 1, zero());
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.status, "covered");
    assert_eq!(u.remaining, bond);
    assert_eq!(u.advisories[0].state, "patched");
}

#[test]
#[should_panic(expected = "patch deadline has not been missed")]
fn cannot_claim_before_deadline() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::Critical);
    at("alice", T0 + 10 + WINDOW, zero());
    c.claim("cam-1".into(), id);
}

#[test]
fn missed_deadline_pays_owner_by_severity() {
    let (mut c, bond) = setup();
    let id = file(&mut c, T0 + 10, Severity::Critical);
    at("alice", T0 + 11 + WINDOW, zero());
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.status, "breach_claimable");
    let half = NearToken::from_yoctonear(bond.as_yoctonear() / 2);
    assert_eq!(u.advisories[0].claimable, half);
    assert_eq!(c.claim("cam-1".into(), id), half);
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.remaining, bond.saturating_sub(half));
    assert_eq!(u.advisories[0].state, "breach_claimed");
    assert_eq!(u.status, "covered");
}

#[test]
#[should_panic(expected = "already claimed")]
fn no_double_claim() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("alice", T0 + 11 + WINDOW, zero());
    c.claim("cam-1".into(), id);
    c.claim("cam-1".into(), id);
}

#[test]
#[should_panic(expected = "only the current owner can claim")]
fn stranger_cannot_claim() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("mallory", T0 + 11 + WINDOW, zero());
    c.claim("cam-1".into(), id);
}

#[test]
fn claim_right_follows_the_nft() {
    let (mut c, bond) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    // Alice sells the camera (and its passport) to Bob while the advisory is open.
    at("alice", T0 + 20, NearToken::from_yoctonear(1));
    c.nft_transfer(acc("bob"), "cam-1".into(), None, None);
    at("bob", T0 + 11 + WINDOW, zero());
    let paid = c.claim("cam-1".into(), id);
    assert_eq!(paid, NearToken::from_yoctonear(bond.as_yoctonear() / 4));
    assert_eq!(c.get_unit("cam-1".into()).unwrap().owner_id, acc("bob"));
}

#[test]
#[should_panic(expected = "only the current owner can claim")]
fn seller_loses_claim_after_transfer() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("alice", T0 + 20, NearToken::from_yoctonear(1));
    c.nft_transfer(acc("bob"), "cam-1".into(), None, None);
    at("alice", T0 + 11 + WINDOW, zero());
    c.claim("cam-1".into(), id);
}

#[test]
fn late_patch_still_owes_the_owner() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::Low);
    at("maker", T0 + 11 + WINDOW, zero());
    c.publish_patch(id, "1.0.1".into(), FIX.into());
    assert_eq!(c.get_advisory(id).unwrap().status, AdvisoryStatus::PatchedLate);
    at("alice", T0 + 12 + WINDOW, zero());
    assert!(!c.claim("cam-1".into(), id).is_zero());
}

#[test]
fn payouts_never_exceed_the_bond() {
    let (mut c, bond) = setup();
    let ids: Vec<u64> = (0..3).map(|i| file(&mut c, T0 + 10 + i, Severity::Critical)).collect();
    at("alice", T0 + 20 + WINDOW, zero());
    let u = c.get_unit("cam-1".into()).unwrap();
    let previewed: u128 = u.advisories.iter().map(|a| a.claimable.as_yoctonear()).sum();
    assert_eq!(previewed, bond.as_yoctonear(), "view previews exactly the bond");
    let mut total = 0u128;
    total += c.claim("cam-1".into(), ids[0]).as_yoctonear();
    total += c.claim("cam-1".into(), ids[1]).as_yoctonear();
    assert_eq!(total, bond.as_yoctonear());
    assert!(c.get_unit("cam-1".into()).unwrap().remaining.is_zero());
}

#[test]
#[should_panic(expected = "bond is exhausted")]
fn exhausted_bond_rejects_further_claims() {
    let (mut c, _) = setup();
    let ids: Vec<u64> = (0..3).map(|i| file(&mut c, T0 + 10 + i, Severity::Critical)).collect();
    at("alice", T0 + 20 + WINDOW, zero());
    c.claim("cam-1".into(), ids[0]);
    c.claim("cam-1".into(), ids[1]);
    c.claim("cam-1".into(), ids[2]);
}

#[test]
#[should_panic(expected = "advisory does not cover this unit")]
fn advisory_before_mint_does_not_cover_unit() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("maker", T0 + 50, near(10));
    c.mint_unit("cam-2".into(), "cam".into(), acc("carol"), SBOM.into(), "1.0.1".into(), SUPPORT);
    at("carol", T0 + 11 + WINDOW, zero());
    c.claim("cam-2".into(), id);
}

#[test]
fn rejected_dispute_voids_advisory_and_slashes_reporter() {
    let (mut c, bond) = setup();
    let id = file(&mut c, T0 + 10, Severity::Critical);
    at("maker", T0 + 100, near(1));
    c.dispute(id);
    // The clock is paused: well past the original deadline nothing is claimable.
    at("alice", T0 + 10 * WINDOW, zero());
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.status, "advisory_open");
    assert_eq!(u.advisories[0].state, "disputed");

    at("cert", T0 + 10 * WINDOW, zero());
    c.resolve_dispute(id, false, Some("not reachable in this firmware".into()));
    let adv = c.get_advisory(id).unwrap();
    assert_eq!(adv.status, AdvisoryStatus::Rejected);
    assert!(adv.stake.is_zero() && adv.dispute_fee.is_zero());
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.status, "covered");
    assert_eq!(u.remaining, bond);
}

#[test]
fn upheld_dispute_restarts_the_clock() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("maker", T0 + 100, near(1));
    c.dispute(id);
    at("cert", T0 + 5_000, zero());
    c.resolve_dispute(id, true, None);
    let adv = c.get_advisory(id).unwrap();
    assert_eq!(adv.status, AdvisoryStatus::Open);
    assert_eq!(adv.deadline, T0 + 5_000 + WINDOW);
    assert!(adv.dispute_fee.is_zero(), "fee went to the reporter");
    at("alice", T0 + 5_001 + WINDOW, zero());
    assert!(!c.claim("cam-1".into(), id).is_zero());
}

#[test]
#[should_panic(expected = "already disputed once")]
fn only_one_dispute_per_advisory() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("maker", T0 + 100, near(1));
    c.dispute(id);
    at("cert", T0 + 200, zero());
    c.resolve_dispute(id, true, None);
    at("maker", T0 + 300, near(1));
    c.dispute(id);
}

#[test]
#[should_panic(expected = "only the line's arbiter")]
fn maker_cannot_rule_on_own_dispute() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("maker", T0 + 100, near(1));
    c.dispute(id);
    at("maker", T0 + 200, zero());
    c.resolve_dispute(id, false, None);
}

#[test]
fn reporter_reclaims_stake_after_breach() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("reporter", T0 + 11 + WINDOW, zero());
    assert_eq!(c.reclaim_stake(id), near(1));
    assert!(c.get_advisory(id).unwrap().stake.is_zero());
}

#[test]
#[should_panic(expected = "support period has not ended")]
fn no_early_release() {
    let (mut c, _) = setup();
    at("maker", SUPPORT, zero());
    c.release("cam-1".into());
}

#[test]
fn clean_end_of_support_returns_whole_bond() {
    let (mut c, bond) = setup();
    at("maker", SUPPORT + 1, zero());
    assert_eq!(c.get_unit("cam-1".into()).unwrap().status, "end_of_support");
    let out = c.release("cam-1".into());
    assert!(out.paid_to_owner.is_zero());
    assert_eq!(out.returned_to_maker, bond);
    let u = c.get_unit("cam-1".into()).unwrap();
    assert_eq!(u.status, "released");
    assert_eq!(u.owner_id, acc("alice"), "passport stays with the owner as a record");
}

#[test]
fn release_settles_unclaimed_breach_first() {
    let (mut c, bond) = setup();
    file(&mut c, T0 + 10, Severity::High); // never patched, never claimed
    at("maker", SUPPORT + 1, zero());
    let out = c.release("cam-1".into());
    let quarter = NearToken::from_yoctonear(bond.as_yoctonear() / 4);
    assert_eq!(out.paid_to_owner, quarter);
    assert_eq!(out.returned_to_maker, bond.saturating_sub(quarter));
}

#[test]
#[should_panic(expected = "still pending")]
fn release_blocked_while_advisory_pending() {
    let (mut c, _) = setup();
    file(&mut c, SUPPORT - 10, Severity::High); // deadline lands after support ends
    at("maker", SUPPORT + 1, zero());
    c.release("cam-1".into());
}

#[test]
#[should_panic(expected = "bond already released")]
fn no_claim_after_release() {
    let (mut c, _) = setup();
    let id = file(&mut c, T0 + 10, Severity::High);
    at("maker", SUPPORT + 1, zero());
    c.release("cam-1".into());
    at("alice", SUPPORT + 2, zero());
    c.claim("cam-1".into(), id);
}

#[test]
fn nft_token_carries_live_status() {
    let (mut c, _) = setup();
    file(&mut c, T0 + 10, Severity::High);
    at("alice", T0 + 11 + WINDOW, zero());
    let extra = c.nft_token("cam-1".into()).unwrap().metadata.unwrap().extra.unwrap();
    assert!(extra.contains("\"status\":\"breach_claimable\""), "{extra}");
}
