//! A facet's value list as a section holds it, and the rules for landing its pages.
//!
//! A facet's values are read in pages of [`VALUE_PAGE`] rows by a store job (`Directory::Values`):
//! one read in flight, carrying only the listing's own `type` terms ([`value_type_terms`]), never the
//! selection, so the list is the whole stable one. A page lands APPENDED, in the server's order, and
//! only if it is the page the list is waiting for: the right listing type, the right generation (a
//! type change or a reset moves it) and exactly the row where the list ends. The section keeps the
//! [`VALUE_LISTS_KEPT`] lists opened last; a list let go is read again from its start.
//!
//! A landing also keeps the selection honest. The titles of selected values are refreshed from the
//! rows (the server renames, and localizes some, e.g. audio languages), and once a list is COMPLETE a
//! selected key it does not hold is pruned, because the listing it filters matches nothing. A partial
//! list never prunes: the key may be on a page that has not landed.

use super::filters::{FilterValue, VALUE_LISTS_KEPT};
use super::letters::LETTERS_RETRY_STEPS;
use super::listing_jobs::ValuesAsk;
use super::{Arc, SecState};

/// The rows of one facet's value list that have landed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValueList {
    /// In the server's order.
    pub rows: Vec<FilterValue>,
    /// Rows the server has SENT so far, the ones that could not travel as a value included: the next
    /// window starts here and completeness is judged on it, never on `rows.len()`.
    pub fetched: usize,
    /// The `totalSize` the server reported, when it did.
    pub total: Option<i64>,
    /// Every row is here: the reported total is reached, or a page came back short or empty.
    pub complete: bool,
    /// [`SecState::values_gen`] when the list was started.
    pub generation: u32,
    /// Moves on every landing: the picker's cheap change signal.
    pub revision: u32,
    /// A read of this list failed, and the steps left before it is asked again.
    pub failed: Option<u32>,
}

impl ValueList {
    /// Append a window: `rows` are the ones of its `fetched` rows that could be offered, `size` is
    /// what was asked for. Decide whether the list is complete.
    fn append(&mut self, rows: Vec<FilterValue>, fetched: usize, total: Option<i64>, size: usize) {
        self.rows.extend(rows);
        self.fetched += fetched;
        self.total = total.or(self.total);
        // A reported total decides; without one a window that came back short ends the list, and an
        // empty one always does (nothing would ever advance past it). Counted on the rows the server
        // sent, so a skipped row is neither a gap in the next window nor a short page.
        self.complete = fetched == 0 || match self.total {
            Some(total) => self.fetched as i64 >= total,
            None => fetched < size,
        };
        self.failed = None;
        self.revision = self.revision.wrapping_add(1);
    }
}

/// What a landing changed in the selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Landed {
    /// A complete list did not hold a selected key: it was dropped, and the listing that key
    /// filtered must be asked afresh.
    pub(super) pruned: bool,
}

impl SecState {
    /// The landed list of `facet`, if any.
    pub(super) fn value_list(&self, facet: &str) -> Option<&Arc<ValueList>> {
        self.values.iter().find(|(name, _)| name == facet).map(|(_, list)| list)
    }

    /// Has the genre list landed whole? The old "genres_done".
    #[cfg(test)]
    pub(super) fn genres_done(&self) -> bool {
        self.value_list("genre").is_some_and(|list| list.complete)
    }

    /// The first row `facet`'s list still needs, or `None` when nothing is owed: it is complete, a
    /// failed read holds it back, or `want_rows` rows are on hand.
    pub(super) fn values_owed(&self, facet: &str, want_rows: usize) -> Option<usize> {
        match self.value_list(facet) {
            None => Some(0),
            Some(list) if list.complete || list.failed.is_some() || list.rows.len() >= want_rows => None,
            Some(list) => Some(list.fetched),
        }
    }

    /// Is a failed value read counting down? The store must be stepped while it is.
    pub(super) fn values_waiting(&self) -> bool {
        self.values.iter().any(|(_, list)| list.failed.is_some())
    }

    /// One step of every back-off.
    pub(super) fn tick_values_backoff(&mut self) {
        for (_, list) in &mut self.values {
            if let Some(steps) = list.failed {
                let list = Arc::make_mut(list);
                list.failed = steps.checked_sub(1).filter(|left| *left > 0);
                list.revision = list.revision.wrapping_add(1);
            }
        }
    }

    /// `facet` was opened or asked for: it is the most recent.
    pub(super) fn touch_values(&mut self, facet: &str) {
        if let Some(at) = self.values.iter().position(|(name, _)| name == facet) {
            let entry = self.values.remove(at);
            self.values.insert(0, entry);
        }
    }

    /// Forget every list: a type change moves the generation, so a read still out lands nowhere.
    pub(super) fn forget_values(&mut self) {
        self.values.clear();
        self.values_gen = self.values_gen.wrapping_add(1);
    }

    /// Install `rows` as `facet`'s complete-or-not list outright (the restore's genre vouch brings one
    /// with the first page).
    pub(super) fn install_values(&mut self, facet: &str, rows: Vec<FilterValue>, fetched: usize, total: Option<i64>, size: usize) {
        if self.value_list(facet).is_some() {
            return;
        }
        let mut list = ValueList { generation: self.values_gen, ..ValueList::default() };
        list.append(rows, fetched, total, size);
        self.values.insert(0, (facet.to_owned(), Arc::new(list)));
        self.values.truncate(VALUE_LISTS_KEPT);
    }

    /// A read of `ask` came back with `rows` (`ok`: the server answered). Refused unless it is the page
    /// the list waits for. Returns what it changed in the selection.
    pub(super) fn land_values(&mut self, ask: ValuesAsk, rows: Vec<FilterValue>, ok: bool) -> Landed {
        if ask.generation != self.values_gen {
            return Landed::default();
        }
        let at = self.values.iter().position(|(name, _)| *name == ask.facet);
        let have = at.map_or(0, |at| self.values[at].1.fetched);
        if ask.start != have {
            return Landed::default();
        }
        let mut list = match at {
            Some(at) => (*self.values.remove(at).1).clone(),
            None => ValueList { generation: self.values_gen, ..ValueList::default() },
        };
        if ok {
            list.append(rows, ask.fetched, ask.total, ask.size);
        } else {
            list.failed = Some(LETTERS_RETRY_STEPS);
            list.revision = list.revision.wrapping_add(1);
        }
        let landed = if ok { self.follow_selection(&ask.facet, &list) } else { Landed::default() };
        self.values.insert(0, (ask.facet, Arc::new(list)));
        self.values.truncate(VALUE_LISTS_KEPT);
        landed
    }

    /// Refresh the titles of the selected values of `facet` from `list`, and drop the keys a COMPLETE
    /// list does not hold.
    fn follow_selection(&mut self, facet: &str, list: &ValueList) -> Landed {
        let Some(term) = self.filters.get(facet) else { return Landed::default() };
        let mut values = Vec::with_capacity(term.values.len());
        for chosen in &term.values {
            match list.rows.iter().find(|row| row.id == chosen.id) {
                Some(row) => values.push(row.clone()),
                None if !list.complete => values.push(chosen.clone()),
                None => {}
            }
        }
        let pruned = values.len() != term.values.len();
        let titles_changed = values.iter().zip(&term.values).any(|(new, old)| new.title != old.title);
        if pruned || titles_changed {
            self.filters = Arc::new(self.filters.with(facet, values));
            if pruned {
                self.reset_listing();
            }
        }
        Landed { pruned }
    }
}
