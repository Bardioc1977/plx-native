//! The owner's half of #830: the stamp a profile is published under is the OWNER's
//! (`plex::session::account_stamp`), and it stays one stamp for the life of a sign-in.
//!
//! The cached roster is part of the stamp's input, and it moves without a token change in three ways
//! these tests drive through the real owner transitions:
//!
//! * the picker's `HomeRoster` commit lands after the seat was published (G2: the owner republishes
//!   once, on the DURABLE completion, never at admission, never inside a sign-in);
//! * the admin's id arrives late (the stamp respells; republishing only when the stamp really moved);
//! * a retry's `home/users` fetch fails (G1: an empty answer never replaces a cached roster).
//!
//! The synthetic ids are `id:7` and `id:8`; the golden stamps are the ones `plex::session`'s
//! `stamp_tests` pins.
use super::*;

const OWNER_7: &str = "o1:19afc47bcea99412";

fn token_form() -> String {
    plx_plex::plex::grant::account_key("synthetic-account")
}

fn admin_ref(id: i64, uuid: &str) -> plx_plex::plex::session::HomeUserRef {
    plx_plex::plex::session::HomeUserRef { id, uuid: uuid.into(), title: "Synthetic".into(), admin: true, ..Default::default() }
}

fn admin_tile(id: i64, uuid: &str) -> UserTile {
    UserTile { id, uuid: uuid.into(), title: "Synthetic".into(), admin: true, ..Default::default() }
}

fn publications(effects: &[SessionFx]) -> Vec<ProfilePublication> {
    effects.iter().filter_map(|fx| match fx { SessionFx::PublishProfile(p) => Some(p.clone()), _ => None }).collect()
}

fn commit_of(effects: &[SessionFx]) -> (u32, u64, u64, CommitPlan) {
    effects.iter().find_map(|fx| match fx {
        SessionFx::Commit { req, epoch, arrival, plan } => Some((*req, *epoch, *arrival, plan.clone())),
        _ => None,
    }).expect("the step issued a commit")
}

fn admit(owner: &mut SessionMachine, commit: &(u32, u64, u64, CommitPlan), revision: u64) -> Vec<SessionFx> {
    let (req, epoch, arrival, plan) = commit;
    step(owner, SessionEvent::Commit(CommitReply { req: *req, epoch: *epoch, arrival: *arrival,
        admission: CommitAdmission::Admitted { revision, purpose: plan.purpose } }))
}

fn completion(commit: &(u32, u64, u64, CommitPlan), revision: u64, durable: bool) -> plx_plex::plex::session::async_persistence::PersistenceCompletion {
    use plx_plex::plex::session::async_persistence::{CompletionOutcome, Failure, Operation, PersistOutcome, PersistenceCompletion};
    let (req, epoch, arrival, plan) = commit;
    PersistenceCompletion { req: *req, epoch: *epoch, arrival: *arrival, revision, purpose: plan.purpose,
        outcome: if durable {
            CompletionOutcome::Durable(Operation::Write { outcome: PersistOutcome::PersistedPlaintext, verified: true, protection: None })
        } else { CompletionOutcome::Failed(Failure::Persistence(PersistOutcome::WriteFailed)) } }
}

/// A signed-in owner with the profile `u-1` seated and `roster` cached: what the Boot picker's
/// refresh starts from. (The Boot picker does not detach the seated profile, which is the only
/// shape in which a roster commit can arrive for a seat that is already published.)
fn seated_owner(roster: Vec<plx_plex::plex::session::HomeUserRef>) -> SessionMachine {
    let mut init = local_session();
    init.persisted.user.uuid = "u-1".into();
    init.persisted.home_users = roster;
    init.committed_credentials = CredentialPatch::of(&init.persisted);
    init.disk_identity = Identity::of(&init.persisted);
    init.active_profile = Some(init.persisted.user.clone());
    SessionMachine::from_init(init)
}

/// The Boot picker asks plex.tv for the roster and gets `users`: returns the roster step's effects
/// (the commit that carries the roster).
fn picker_roster(owner: &mut SessionMachine, users: Vec<UserTile>) -> Vec<SessionFx> {
    let effects = step(owner, SessionEvent::Command(Command::StartSwitch(Picker::Boot)));
    let (req, key) = effects.iter().find_map(|fx| match fx {
        SessionFx::Work { req, key, input: SessionWork::HomeRoster { .. }, .. } => Some((*req, *key)),
        _ => None,
    }).expect("the Boot picker fetches the Home roster");
    settle_picker_commit(owner, &effects);
    let envelope = roster_envelope(owner, req, key, Some(users));
    step(owner, SessionEvent::Result(envelope))
}

/// A seated owner whose roster commit has been ADMITTED and whose durable completion is still
/// withheld (the storage worker is parked): the state G2 must not publish from.
fn parked_roster_commit(before: Vec<plx_plex::plex::session::HomeUserRef>, users: Vec<UserTile>)
    -> (SessionMachine, (u32, u64, u64, CommitPlan)) {
    let mut owner = seated_owner(before);
    let commit = commit_of(&picker_roster(&mut owner, users));
    let at_admission = admit(&mut owner, &commit, 7);
    assert!(publications(&at_admission).is_empty(), "rig: nothing is published at admission");
    assert!(owner.state.admitted_persistence.is_some(), "rig: the owner waits for the durable verdict");
    (owner, commit)
}

/// **S-13. A roster landing after the seat republishes ONCE, on the durable completion.** The seat
/// was published under the token form (the owner was unknown); the picker's roster makes it known.
/// The credential write is admitted first and lands later on the storage worker, and the restore a
/// publication triggers reads what is on disk, so publishing at admission would restore from the
/// old pair. Nothing is published at admission or while the worker is parked, exactly one
/// publication follows the durable completion, and it carries the owner stamp.
#[test]
fn a_roster_landing_after_the_seat_republishes_once_on_the_durable_completion() {
    let _g = plx_base::testlock::serial();
    let (mut owner, commit) = parked_roster_commit(Vec::new(), vec![admin_tile(7, "synthetic-owner-7")]);
    assert_eq!(owner.state.persisted.home_users.len(), 1, "the roster is merged at admission");
    assert_eq!(owner.state.active_profile.as_ref().map(|u| u.uuid.as_str()), Some("u-1"), "the profile is seated");

    // The worker is parked: unrelated traffic publishes nothing either.
    assert!(publications(&step(&mut owner, SessionEvent::Command(Command::NoteDeleteLeftovers(0)))).is_empty());

    let published = publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true))));
    assert_eq!(published.len(), 1, "exactly one republication after the durable completion");
    assert_eq!(published[0].stamp, OWNER_7, "carrying the owner stamp");
    assert_eq!(published[0].profile.as_ref().map(|u| u.uuid.as_str()), Some("u-1"), "for the profile that was seated");
    assert_eq!(plx_plex::plex::session::account_stamp(&owner.state.persisted), OWNER_7);

    let duplicate = publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true))));
    assert!(duplicate.is_empty(), "a duplicate completion publishes nothing");
}

/// A completion that is not a durable verdict for THIS write publishes nothing: a failed write
/// leaves the seat on the stamp that still matches the disk, a stale revision settles nothing.
#[test]
fn only_a_durable_completion_of_the_admitted_write_republishes_the_seat() {
    let _g = plx_base::testlock::serial();
    let (mut owner, commit) = parked_roster_commit(Vec::new(), vec![admin_tile(7, "synthetic-owner-7")]);
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 99, true)))).is_empty(),
        "another revision's verdict is inert");
    assert!(owner.state.admitted_persistence.is_some(), "and does not consume the wait");
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, false)))).is_empty(),
        "a write that did not land republishes nothing");
}

/// The republication re-checks the world at completion time, because the verdict can arrive after
/// the owner moved on: a sign-out empties the stamp, an authorization in flight owns the next
/// publication (the final commit's), and a held handoff is released by its own completion only.
#[test]
fn the_seat_is_republished_only_while_the_completion_still_describes_a_seated_stable_session() {
    let _g = plx_base::testlock::serial();
    let users = || vec![admin_tile(7, "synthetic-owner-7")];

    let (mut owner, commit) = parked_roster_commit(Vec::new(), users());
    step(&mut owner, SessionEvent::Command(Command::SignOut));
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true)))).is_empty(),
        "after a sign-out the stamp is empty: nothing to seat");

    let (mut owner, commit) = parked_roster_commit(Vec::new(), users());
    owner.state.active_profile = None;
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true)))).is_empty(),
        "no profile seated: nothing to republish");

    let (mut owner, commit) = parked_roster_commit(Vec::new(), users());
    owner.state.authorized_in_flow = true;
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true)))).is_empty(),
        "a sign-in in flight publishes through its own final commit");

    let (mut owner, commit) = parked_roster_commit(Vec::new(), users());
    owner.state.held_handoff = Some(HeldHandoff { epoch: 99, req: 99 });
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true)))).is_empty(),
        "an unrelated held handoff is not bypassed");
    assert!(owner.state.held_handoff.is_some(), "and is still held");
}

/// The owner keeps ONE slot for the write it waits on (`admitted_persistence`), overwritten by
/// every admission. A republication whose completion was overtaken by a later admission is dropped
/// with it: the seat keeps the stamp it has, which is the safe direction (the records of the old
/// stamp are still on the seat's side of Invariant I), and the next restore converges.
#[test]
fn a_republication_overtaken_by_a_later_admission_is_dropped_fail_closed() {
    let _g = plx_base::testlock::serial();
    let (mut owner, commit) = parked_roster_commit(Vec::new(), vec![admin_tile(7, "synthetic-owner-7")]);
    let mut later = owner.state.admitted_persistence.expect("rig");
    later.revision = 8;
    later.req = later.req.wrapping_add(1);
    owner.state.admitted_persistence = Some(later);
    assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true)))).is_empty());
}

/// **S-15. The admin's id arriving late republishes ONCE.** An old file names the owner by uuid;
/// a refresh brings the id of the same admin, which respells the stamp. A further identical roster
/// moves nothing and republishes nothing (the decision is on the STAMP, not on the roster).
#[test]
fn a_late_admin_id_republishes_once_without_orphaning_the_uuid_stamped_records() {
    let _g = plx_base::testlock::serial();
    let uuid_only = vec![admin_ref(0, "synthetic-owner-7")];
    let mut owner = seated_owner(uuid_only);
    let uuid_stamp = plx_plex::plex::session::account_stamp(&owner.state.persisted);

    let commit = commit_of(&picker_roster(&mut owner, vec![admin_tile(7, "synthetic-owner-7")]));
    admit(&mut owner, &commit, 7);
    let once = publications(&step(&mut owner, SessionEvent::Persistence(completion(&commit, 7, true))));
    assert_eq!(once.len(), 1, "the respelled stamp is republished once");
    assert_eq!(once[0].stamp, OWNER_7);
    assert_eq!(uuid_stamp, "o1:82eac2ef2031e0d2", "it moved from the uuid spelling of the same admin");

    // The same roster again: the stamp does not move, so nothing is written or published.
    let effects = picker_roster(&mut owner, vec![admin_tile(7, "synthetic-owner-7")]);
    assert!(publications(&effects).is_empty());
    if let Some(again) = effects.iter().find_map(|fx| match fx {
        SessionFx::Commit { req, epoch, arrival, plan } => Some((*req, *epoch, *arrival, plan.clone())),
        _ => None,
    }) {
        admit(&mut owner, &again, 8);
        assert!(publications(&step(&mut owner, SessionEvent::Persistence(completion(&again, 8, true)))).is_empty(),
            "an unchanged stamp is never republished");
    }
}

/// **S-13a. A re-sign-in with a seated profile publishes nothing before the final commit.**
/// `Authorized` already moved the owner's `persisted` to the new token with no roster, while the
/// previous account's profile is still the active one. The discovery commit that follows makes the
/// owner known, and a republication there would seat the PREVIOUS account's profile under the NEW
/// owner stamp and bypass the held handoff. Only the final commit's handoff publishes, once, under
/// the owner stamp of the new sign-in. (Run twice: the first answer, and a retry after the
/// authorization, which keeps `authorized_in_flow`.)
#[test]
fn a_resignin_with_a_seated_profile_publishes_nothing_before_the_final_commit() {
    use crate::auth::LoginProgress;
    let _g = plx_base::testlock::serial();
    for retry in [false, true] {
        let mut owner = seated_owner(vec![admin_ref(7, "synthetic-owner-7")]);
        let published = std::cell::RefCell::new(Vec::<ProfilePublication>::new());
        let run = |owner: &mut SessionMachine, event: SessionEvent| {
            let effects = step(owner, event);
            published.borrow_mut().extend(publications(&effects));
            effects
        };
        assert!(owner.restart_login(true, &mut |_| {}));
        let (epoch, req) = (owner.state.epoch, owner.state.next_req);
        for (arrival, progress) in [
            (1, LoginProgress::CodeReady { epoch, code: "synthetic-code".into(), qr_png: vec![1] }),
            (2, LoginProgress::Authorized { epoch, token: "synthetic-new-token".into() }),
        ] {
            let event = qr_event(&owner, req, arrival, progress, false);
            run(&mut owner, SessionEvent::Result(event));
        }
        assert!(owner.state.persisted.home_users.is_empty(), "rig: Authorized emptied the roster");
        assert!(owner.state.active_profile.is_some(), "rig: the previous account's profile is still the active one");
        let (req, epoch, arrival) = if retry {
            run(&mut owner, SessionEvent::Command(Command::Retry));
            (owner.state.next_req, owner.state.epoch, 1)
        } else { (req, epoch, 3) };
        assert!(owner.state.authorized_in_flow, "rig: the authorization is still in flight");
        let signed_in = qr_event(&owner, req, arrival, LoginProgress::SignedIn { epoch, server: local_server(),
            sources: Vec::new(), users: vec![admin_tile(7, "synthetic-owner-7")] }, true);
        let discovery = commit_of(&run(&mut owner, SessionEvent::Result(signed_in)));
        assert_eq!(discovery.3.purpose, PersistencePurpose::Discovery, "rig: this is the sign-in's discovery save");
        run(&mut owner, SessionEvent::Commit(CommitReply { req: discovery.0, epoch: discovery.1, arrival: discovery.2,
            admission: CommitAdmission::Admitted { revision: 1, purpose: PersistencePurpose::Discovery } }));
        run(&mut owner, SessionEvent::Persistence(completion(&discovery, 1, true)));
        assert!(published.borrow().is_empty(), "retry={retry}: the discovery commit publishes nothing, found {}", published.borrow().len());

        let final_commit = commit_of(&run(&mut owner, SessionEvent::Command(Command::TakeReady)));
        assert_eq!(final_commit.3.purpose, PersistencePurpose::Final, "rig: the final save");
        run(&mut owner, SessionEvent::Commit(CommitReply { req: final_commit.0, epoch: final_commit.1, arrival: final_commit.2,
            admission: CommitAdmission::Admitted { revision: 2, purpose: PersistencePurpose::Final } }));
        assert!(published.borrow().is_empty(), "retry={retry}: the final write is admitted and its handoff held");
        run(&mut owner, SessionEvent::Persistence(completion(&final_commit, 2, true)));
        assert_eq!(published.borrow().len(), 1, "retry={retry}: the held handoff publishes exactly once");
        assert_eq!(published.borrow()[0].stamp, OWNER_7, "retry={retry}: under the owner stamp of the new sign-in");
    }
}

/// **S-14. A failed `home/users` fetch never blanks a roster cached for the same token.** A retry
/// after the authorization (`Rediscover`) whose fetch failed arrives as `SignedIn` with no users;
/// assigned as it stands it erased the roster the first answer of this very token had committed,
/// and with it the owner (the stamp would fall back to the token form mid-sign-in). A NEW sign-in's
/// empty answer stays empty: `Authorized` had already cleared the old account's roster.
#[test]
fn a_failed_home_users_fetch_on_rediscover_keeps_the_cached_roster() {
    use crate::auth::LoginProgress;
    let _g = plx_base::testlock::serial();
    let rig = |op: SessionOp, roster: Vec<plx_plex::plex::session::HomeUserRef>| {
        let mut init = discovering_after_authorization();
        init.persisted.home_users = roster;
        for pending in init.pending.values_mut() {
            pending.key.op = op;
            pending.expected = Identity::of(&init.persisted);
        }
        SessionMachine::from_init(init)
    };
    let signed_in_empty = |owner: &SessionMachine| {
        let (req, epoch) = (owner.state.next_req, owner.state.epoch);
        qr_event(owner, req, 1, LoginProgress::SignedIn { epoch, server: local_server(), sources: Vec::new(), users: Vec::new() }, true)
    };

    let mut owner = rig(SessionOp::Rediscover, vec![admin_ref(7, "synthetic-owner-7")]);
    let event = signed_in_empty(&owner);
    let commit = commit_of(&step(&mut owner, SessionEvent::Result(event)));
    let patch = commit.3.credentials.as_ref().expect("the discovery save carries credentials");
    assert_eq!(patch.home_users.len(), 1, "the write does not blank the cached roster");
    admit(&mut owner, &commit, 1);
    assert_eq!(owner.state.persisted.home_users.len(), 1, "and neither does the owner's copy");
    assert_eq!(plx_plex::plex::session::account_stamp(&owner.state.persisted), OWNER_7, "so the owner is still known");

    let mut owner = rig(SessionOp::Login, Vec::new());
    let event = signed_in_empty(&owner);
    let commit = commit_of(&step(&mut owner, SessionEvent::Result(event)));
    admit(&mut owner, &commit, 1);
    assert!(owner.state.persisted.home_users.is_empty(), "a new sign-in's empty answer is what it was given");
    assert_eq!(plx_plex::plex::session::account_stamp(&owner.state.persisted), token_form());
}

/// **S-3. A roster fetched under the old token never pairs with the new one.** `Authorized` moves
/// the owner to (new token, no roster); the stamp is the token form until a roster of THIS token
/// lands, and an answer to a question asked under the old token is dropped.
#[test]
fn a_roster_fetched_under_the_old_token_never_pairs_with_the_new_one() {
    use crate::auth::LoginProgress;
    let _g = plx_base::testlock::serial();
    let mut owner = seated_owner(vec![admin_ref(7, "synthetic-owner-7")]);
    let effects = step(&mut owner, SessionEvent::Command(Command::StartSwitch(Picker::Boot)));
    let (stale_req, stale_key) = effects.iter().find_map(|fx| match fx {
        SessionFx::Work { req, key, input: SessionWork::HomeRoster { .. }, .. } => Some((*req, *key)),
        _ => None,
    }).expect("the Boot picker fetches the roster under the old token");
    settle_picker_commit(&mut owner, &effects);
    let old_identity = crate::auth::SessionIdentity::of(&owner.state.persisted);

    assert!(owner.restart_login(true, &mut |_| {}));
    let (epoch, req) = (owner.state.epoch, owner.state.next_req);
    for (arrival, progress) in [
        (1, LoginProgress::CodeReady { epoch, code: "synthetic-code".into(), qr_png: vec![1] }),
        (2, LoginProgress::Authorized { epoch, token: "synthetic-new-token".into() }),
    ] {
        let event = qr_event(&owner, req, arrival, progress, false);
        step(&mut owner, SessionEvent::Result(event));
    }
    assert_eq!(owner.state.persisted.account_token, "synthetic-new-token");
    assert!(owner.state.persisted.home_users.is_empty(), "the old account's roster is gone with its token");
    assert_eq!(plx_plex::plex::session::account_stamp(&owner.state.persisted),
        plx_plex::plex::grant::account_key("synthetic-new-token"), "the token form until a roster of this token lands");

    let stale = SessionEnvelope {
        addr: Addr { to: MachineId::Session, req: plx_machine::machine::RequestId(stale_req) },
        key: stale_key, admission: AdmissionId(stale_req), arrival: 1, terminal: true, lifecycle: None,
        outcome: SessionArrival::Data(Arc::new(crate::auth::observation::Observation::HomeRoster(
            crate::auth::HomeRosterProgress { epoch: stale_key.epoch, expected: old_identity,
                users: Some(vec![admin_tile(7, "synthetic-owner-7")]) }))),
    };
    let effects = step(&mut owner, SessionEvent::Result(stale));
    assert!(!effects.iter().any(|fx| matches!(fx, SessionFx::Commit { .. })), "the old token's answer is not committed");
    assert!(owner.state.persisted.home_users.is_empty());
}

/// The publication carries `account_stamp(persisted)`: the owner form for a roster that names its
/// admin, the token form without one, nothing without a token. A different owner's stamp differs.
#[test]
fn the_published_stamp_is_the_account_stamp_of_the_persisted_session() {
    let _g = plx_base::testlock::serial();
    let publish = |roster: Vec<plx_plex::plex::session::HomeUserRef>, token: &str| {
        let mut owner = seated_owner(roster);
        owner.state.persisted.account_token = token.into();
        let mut effects = Vec::new();
        owner.publish_profile(Some(owner.state.persisted.user.clone()), &mut |fx| effects.push(fx));
        publications(&effects).pop().expect("a publication").stamp
    };
    assert_eq!(publish(vec![admin_ref(7, "synthetic-owner-7")], "synthetic-token-a"), OWNER_7);
    assert_eq!(publish(vec![admin_ref(7, "synthetic-owner-7")], "synthetic-token-b"), OWNER_7, "a new token: the same stamp");
    assert_ne!(publish(vec![admin_ref(8, "synthetic-owner-8")], "synthetic-token-a"), OWNER_7, "another owner: another stamp");
    assert_eq!(publish(Vec::new(), "synthetic-token-a"), plx_plex::plex::grant::account_key("synthetic-token-a"));
    assert_eq!(publish(vec![admin_ref(7, "synthetic-owner-7")], ""), "");
}
