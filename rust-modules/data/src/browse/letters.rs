//! The A-Z rail's first-letter list, and what it counted.
//!
//! The rail jumps to a letter by the running sum of the counts before it, so the counts have to
//! be those of the very listing the grid shows: the same `type=`, the same Unwatched key
//! (`unwatched` or `unwatchedLeaves`) and the same genre, sorted by title. The letters read
//! therefore carries the terms of the grid's own page read ([`SecState::query_filters`], the one
//! source of both), and a landed list is KEYED by what it counted ([`LettersKey`]).
//!
//! A list is shown only while its key is the one the section's present query would ask with
//! ([`SecState::shown_letters`]), and that is checked where the letters are READ, not at the
//! edit sites: the first page's landing applies a saved genre after the letters were asked
//! (`pump_owned_with_gate`), and nothing there has to remember to forget them. Two things move the
//! key without a term changing and both bump [`LettersKey::gen`]: the server's membership changed
//! under the section ([`SecState::membership_changed`] and the stale flag spent by
//! [`SecState::reset_listing`]), and, by construction, nothing else. A library TYPE change forgets
//! the list outright ([`SecState::forget_letters`]); the Collections page's loan does not carry it.
//!
//! A read that FAILED leaves the list as it was (not current, so hidden: never shown over a grid it
//! does not describe) and holds the same key back for [`LETTERS_RETRY_STEPS`] steps, as the page
//! lane's `retry_cd` does; any edit asks at once.

use super::{Arc, BrowseState, DirectoryResult, SecState};

/// What a first-letter list counted: the grid's own query as it stood when the list was asked.
/// Compared field by field wherever the letters are read ([`LettersKey::is`]), without allocating.
///
/// There is deliberately no listing type here: the landing refuses an answer whose
/// `DirectoryResult::library_type` is not the section's, and a type change forgets the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LettersKey {
    /// The Unwatched switch AS APPLIED (false while a type the filters do not apply to is listed).
    pub(super) unwatched: bool,
    /// The genre tag id AS APPLIED.
    pub(super) genre: Option<String>,
    /// [`SecState::letters_gen`] when asked: moves when the server's membership changed under the
    /// section, which no term shows.
    pub(super) gen: u32,
}

impl LettersKey {
    fn is(&self, unwatched: bool, genre: Option<&str>, gen: u32) -> bool {
        self.unwatched == unwatched && self.genre.as_deref() == genre && self.gen == gen
    }
}

/// Steps a failed letters read holds the same key back (the page lane's `retry_cd` is 120 too).
pub(super) const LETTERS_RETRY_STEPS: u32 = 120;

impl SecState {
    /// The terms the section's query applies now: Unwatched and the genre's tag id, both `off` while a
    /// type the filters do not apply to is listed ([`super::LibraryType::filters`]).
    fn applied_terms(&self) -> (bool, Option<&str>) {
        if !self.library_type.filters() {
            return (false, None);
        }
        (self.unwatched, self.genre.as_ref().map(|genre| genre.id.as_str()))
    }

    /// The key a letters read asked NOW would carry.
    pub(super) fn letters_key(&self) -> LettersKey {
        let (unwatched, genre) = self.applied_terms();
        LettersKey { unwatched, genre: genre.map(str::to_owned), gen: self.letters_gen }
    }

    /// The list that describes the listing the grid shows now, if one is on hand: the landed one,
    /// or the one it replaced ([`Self::land_letters`]). Runs every Library step and every snapshot,
    /// so it compares in place, and it is the one place a list is picked: every reader goes
    /// through it, so nothing has to remember to swap a list back.
    pub(super) fn shown_letters(&self) -> Option<&Arc<Vec<(String, i64)>>> {
        let (unwatched, genre) = self.applied_terms();
        if self.letters_for.as_ref().is_some_and(|key| key.is(unwatched, genre, self.letters_gen)) {
            return Some(&self.letters);
        }
        self.letters_kept
            .as_ref()
            .filter(|(key, _)| key.is(unwatched, genre, self.letters_gen))
            .map(|(_, list)| list)
    }

    /// Does a list on hand describe the listing the grid shows now?
    pub(super) fn letters_current(&self) -> bool {
        self.shown_letters().is_some()
    }

    /// Is a letters read owed: the list is not current, and a failed read of THIS key is not
    /// holding the next one back?
    pub(super) fn letters_owed(&self) -> bool {
        if self.letters_current() {
            return false;
        }
        let (unwatched, genre) = self.applied_terms();
        !self.letters_backoff.as_ref().is_some_and(|(key, steps)| *steps > 0 && key.is(unwatched, genre, self.letters_gen))
    }

    /// Is a back-off counting down? The store must be stepped while it is, as for `retry_cd`.
    pub(super) fn letters_waiting(&self) -> bool {
        self.letters_backoff.is_some()
    }

    /// One step of the back-off; spent at zero.
    pub(super) fn tick_letters_backoff(&mut self) {
        if let Some((_, steps)) = &mut self.letters_backoff {
            *steps = steps.saturating_sub(1);
            if *steps == 0 {
                self.letters_backoff = None;
            }
        }
    }

    /// A read asked under `key` came back with `list`. It becomes the landed list, and is shown only
    /// if `key` is still the section's ([`Self::shown_letters`]): an answer asked under older terms is
    /// stored but never current, so the next step asks again.
    ///
    /// The list it replaces is KEPT, one slot (so the last two keys are held, never more), unless it
    /// was counted under Unwatched: turning a genre or Unwatched on and then off again finds the
    /// unfiltered counts where they were instead of reading them anew. An Unwatched list is the
    /// exception because which titles it counts changes with every watch, and a watch made while the
    /// switch is OFF flags nothing ([`SecState::membership_depends_on_viewing`]), so a kept one
    /// could come back wrong; its key's `gen` is the only thing that would tell. A list a
    /// membership change or a type change made unshowable ([`Self::membership_changed`],
    /// [`Self::forget_letters`]) is never brought back: its key carries the old `gen`, or it is gone.
    pub(super) fn land_letters(&mut self, key: LettersKey, list: Vec<(String, i64)>) {
        match self.letters_for.take() {
            Some(previous) if previous != key => {
                let replaced = std::mem::replace(&mut self.letters, Arc::new(list));
                self.letters_kept = (!previous.unwatched).then_some((previous, replaced));
            }
            _ => {
                self.letters = Arc::new(list);
                if self.letters_kept.as_ref().is_some_and(|(kept, _)| *kept == key) {
                    self.letters_kept = None;
                }
            }
        }
        self.letters_for = Some(key);
        self.letters_backoff = None;
    }

    /// A read asked under `key` FAILED: the list stays as it was and the same key waits.
    fn letters_failed(&mut self, key: LettersKey) {
        self.letters_backoff = Some((key, LETTERS_RETRY_STEPS));
    }

    /// Drop the list and everything about it. Runs on EVERY library type change, entering and
    /// leaving Collections included: counts describe one metadata type, and the loan holds none.
    pub(super) fn forget_letters(&mut self) {
        self.letters = Arc::default();
        self.letters_for = None;
        self.letters_kept = None;
        self.letters_backoff = None;
    }

    /// What the server lists under this section changed (a library scan, a read-back that moved a
    /// row's membership) though no term did: every offset after the change is off by one until the
    /// counts are asked again, and the listing is forgotten as a query edit forgets it.
    ///
    /// A change that covers no LOADED row of the current section (a title in a page not fetched, or
    /// watched from Home) flags nothing, as for the grid's later pages; that blind spot is older than
    /// the rail and unchanged.
    pub(super) fn membership_changed(&mut self) {
        self.letters_gen = self.letters_gen.wrapping_add(1);
        self.reset_listing();
    }
}

impl BrowseState {
    /// Land the letters mailbox's answer. A read that SUCCEEDED replaces the list under the key it
    /// was asked with (an empty answer is a landed list: a filter matching nothing); one that failed
    /// touches nothing but the back-off. `#[track_caller]` for the paced dump, which keys a landing's
    /// claim by CALL SITE, as [`Self::land_directory_owned_with_gate`] says.
    #[track_caller]
    pub(super) fn land_letters_owned(
        &mut self,
        gate: &plx_machine::landgate::Gate,
        fetch: &crate::stores::Fetch<DirectoryResult<(String, i64)>>,
    ) -> bool {
        self.land_directory_owned_with_gate(gate, fetch, |state, list, ok, asked| {
            let Some(asked) = asked else { return };
            if ok {
                state.land_letters(asked, list);
            } else {
                state.letters_failed(asked);
            }
        })
    }

    /// [`Self::land_letters_owned`] on the fixture gate.
    #[cfg(test)]
    #[track_caller]
    pub(super) fn land_letters_for_test(
        &mut self,
        fetch: &crate::stores::Fetch<DirectoryResult<(String, i64)>>,
    ) -> bool {
        self.land_letters_owned(plx_machine::landgate::fixture_gate(), fetch)
    }
}
