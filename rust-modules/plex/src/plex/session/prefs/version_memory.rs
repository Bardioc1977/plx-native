//! **Per-item version memory — the data layer.** Which of an item's `Media[]` versions a profile
//! chose to play, remembered so the next play of that title (from the detail page, a Continue
//! Watching card, Up Next or a link) starts on the same file. This file is only the DATA: the row,
//! the entry, the bounds and the budget. Read at resolve time (`route::recalled_version`) and
//! written by the version picker (`route::remember_version`), both through the memory door, so the
//! key is not serialized until a viewer has picked a version other than the default
//! (`docs/version-selection-spec.md` section 4.3).
//!
//! The list is `Preferences::version_memory`, a [`ProfileList`] per profile held through the memory
//! door (`ProfileMemory::upsert_with`), keyed by `(machine_id, rating_key)`, at most [`CAP`] entries
//! a profile, oldest first. The VALUE is the version's `Media.id`, never its position: a library
//! scan can reorder `Media[]`, and an id that is no longer listed simply plays the default.
//!
//! It is **account-bound** like `track_memory`: the record carries the account stamp
//! (`grant::account_key`), a record stamped for any other account reads as nothing, a write without
//! a stamp is refused, and a write prunes every record that is not its own. It lives in the public
//! half of the DB8 document, so it is **bounded in bytes** ([`BUDGET_BYTES`]); the key strings share
//! `track_memory`'s bounds (a server machine id, a ratingKey) and the one normaliser,
//! [`VersionMemoryEntry::normalized`], applies them when an entry is built and when a file is read.
//! The document guard sheds this list LAST (`prefs::shed_memory`): losing a version pick silently
//! plays a different file, where losing a sort order costs one menu.
//!
//! The wire shape:
//!
//! ```json
//! { "machine_id": "…", "rating_key": "…", "media_id": 7000001 }
//! ```
//!
//! Picking the default version (the one at wire index 0) *forgets* the entry rather than recording
//! it: "the default needs no record", the rule an offset of 0 follows. An entry whose `media_id` is
//! not positive therefore [`forgets`](MemoryEntry::forgets).
use super::track_memory::{drop_if_longer, MACHINE_ID_MAX, SHOW_RK_MAX};
use super::{MemoryEntry, Parse, ProfileList, Row, Scope, SettingsWrite};
use crate::plex::session::Session;
use serde::{Deserialize, Serialize};

/// The most entries one profile keeps (the oldest are dropped first). The owner's library has 50
/// multi-version items, so 128 is room for every one of them to be remembered.
pub const CAP: usize = 128;
/// The household budget for the whole list, in logical bytes (platform's counting rule). 128
/// worst-case entries for one profile fit in it (`one_profile_keeps_128_worst_case_entries` builds
/// them from the bounds), so one viewer always keeps the full [`CAP`]; several heavy viewers share
/// what is left.
pub const BUDGET_BYTES: usize = 24 * 1024;

/// **One item's remembered version for one profile**, keyed by `(machine_id, rating_key)`.
///
/// Deserialization goes through [`VersionMemoryEntryWire`] and [`normalized`](Self::normalized), and
/// refuses an entry that [`forgets`](MemoryEntry::forgets), so `de_soft_vec` drops it: a hand-edited
/// file cannot hold an over-long string, a keyless entry or a version that means "the default".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "VersionMemoryEntryWire")]
pub struct VersionMemoryEntry {
    pub machine_id: String,
    /// The item's `ratingKey` (a movie, or the episode itself: the memory is per episode).
    pub rating_key: String,
    /// The chosen version's `Media.id`. Positive in any entry the list holds.
    pub media_id: i64,
}

/// The tolerant read shape of a [`VersionMemoryEntry`]: the key strings and the id are required
/// (a wrong type fails the element, which `de_soft_vec` then drops alone).
#[derive(Deserialize)]
struct VersionMemoryEntryWire {
    machine_id: String,
    rating_key: String,
    media_id: i64,
}

impl TryFrom<VersionMemoryEntryWire> for VersionMemoryEntry {
    type Error = &'static str;
    fn try_from(wire: VersionMemoryEntryWire) -> Result<Self, Self::Error> {
        let entry = VersionMemoryEntry {
            machine_id: wire.machine_id,
            rating_key: wire.rating_key,
            media_id: wire.media_id,
        }
        .normalized();
        if MemoryEntry::forgets(&entry) {
            Err("a version-memory entry needs a server, an item and a positive Media.id")
        } else {
            Ok(entry)
        }
    }
}

impl VersionMemoryEntry {
    /// An entry as the list may hold it: every string within its bound.
    pub fn new(machine_id: &str, rating_key: &str, media_id: i64) -> Self {
        VersionMemoryEntry { machine_id: machine_id.to_string(), rating_key: rating_key.to_string(), media_id }
            .normalized()
    }

    /// The largest entry the list can hold, the `n`th of its profile: both strings at their bound
    /// and the widest id. 128 of them for one profile are the household budget's size
    /// (`one_profile_keeps_128_worst_case_entries`). Built through [`VersionMemoryEntry::new`].
    pub fn worst_case(n: u32) -> Self {
        let digits = format!("{n:0>width$}", width = SHOW_RK_MAX);
        VersionMemoryEntry::new(&"m".repeat(MACHINE_ID_MAX), &digits, i64::MAX - i64::from(n))
    }

    /// **The one normaliser**: applied when an entry is built, when it is written through
    /// `upsert_with`, and when a file is read. A key string over its bound is dropped, which leaves
    /// the entry keyless and so forgotten.
    pub fn normalized(mut self) -> Self {
        drop_if_longer(&mut self.machine_id, MACHINE_ID_MAX);
        drop_if_longer(&mut self.rating_key, SHOW_RK_MAX);
        self
    }
}

impl MemoryEntry for VersionMemoryEntry {
    type Key = (String, String);
    const CAP: usize = CAP;
    const ACCOUNT_BOUND: bool = true;
    const BUDGET: Option<usize> = Some(BUDGET_BYTES);
    fn key(&self) -> Self::Key {
        (self.machine_id.clone(), self.rating_key.clone())
    }
    fn matches(&self, key: &Self::Key) -> bool {
        self.machine_id == key.0 && self.rating_key == key.1
    }
    /// The default version needs no record, and an item with no machine id or ratingKey is never
    /// recorded (`LastLibrary::set`'s reason).
    fn forgets(&self) -> bool {
        self.machine_id.is_empty() || self.rating_key.is_empty() || self.media_id <= 0
    }
    fn normalized(self) -> Self {
        VersionMemoryEntry::normalized(self)
    }
    fn slot(session: &Session) -> &Vec<ProfileList<Self>> {
        &session.prefs.version_memory
    }
    fn slot_mut(session: &mut Session) -> &mut Vec<ProfileList<Self>> {
        &mut session.prefs.version_memory
    }
}

/// The `version_memory` row: one item, one profile, bound to the account, applied by the resolve
/// (not a live cell), with no Settings row. A [`Row`] and deliberately not a `ListRow`, like
/// `TrackMemoryPref`: it has no whole-list accessor, so it is written only through the memory door.
pub struct VersionMemoryPref;
impl Row for VersionMemoryPref {
    const KEY: &'static str = "version_memory";
    const SCOPE: Scope = Scope::PerItem;
    const ACCOUNT_BOUND: bool = true;
    const LIVE: bool = false;
    const SETTINGS: Option<SettingsWrite> = None;
    const PARSE: Parse = Parse::SoftVec;
}
