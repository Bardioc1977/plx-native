//! **The account stamp** a per-account RECORD carries (`profile_prefs`, `track_memory`,
//! `subtitle_offsets`): which sign-in's data the record is.
//!
//! STUB (tests-first commit): every function here is inert and equals the previous behaviour, so
//! the regression tests that follow fail by assertion on the base behaviour. The next commit
//! replaces the bodies.
use super::Session;

/// The one stamp a per-account record carries.
pub fn account_stamp(session: &Session) -> String {
    super::super::grant::account_key(&session.account_token)
}

/// Whether `stamp` is in the owner family (`o<digits>:...`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn is_owner_stamp(_stamp: &str) -> bool {
    false
}

/// The stamps `session`'s records may be adopted from.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn adoptable_stamps(_session: &Session) -> Vec<String> {
    Vec::new()
}

impl Session {
    /// This session with every adoptable record re-stamped to [`account_stamp`]. STUB: identity.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn adopt_stamps(self) -> Session {
        self
    }
}
