// SPDX-License-Identifier: MIT
//! What a conversation GROUP implies, worked out once and kept beside the index.
//!
//! ## What belongs here
//!
//! Anything calculated that depends on the group and on nothing else - not the world, not the
//! layout, not where a search starts. Such a thing is the same answer in every process that
//! ever opens the group, so every process after the first works it out again for nothing.
//!
//! AND ONLY WHERE RECALCULATING IS SLOWER THAN THE READ, which is a measurement rather than a
//! principle. The first fact kept here takes 14 ms to work out on the heavy groups against 0.10
//! ms to read back, so it earns its file more than a hundred times over. Something that took a
//! microsecond would not, and would belong in neither this module nor a file.
//!
//! ## What makes a stored answer valid, and the hard part of any cache
//!
//! THE INDEX IS ALREADY A CACHE, of the dialogue database, and it already carries what says
//! whether it is still good: a SHA-256 per conversation over exactly the fields the engine
//! reads - see `ConversationHasher` on the C# side - and a format version for the case a
//! content hash cannot catch, an index that really is the same content but predates fields the
//! engine has since started reading. This reuses both rather than inventing a key: a stored
//! answer carries the id and hash of every conversation in its group, and is refused unless
//! every one still matches the index in hand.
//!
//! EVERY MEMBER, because these are facts about a GROUP and a group is several conversations -
//! 640's is eleven. An answer keyed on the conversation that was asked for would survive a
//! change to any of the other ten.
//!
//! A COMPARISON RATHER THAN A DIGEST OF THE KEY. Storing the key and checking it is exact,
//! where a hash of it would trade a silent wrong answer for a smaller file. Wrong here does not
//! announce itself - a stale answer drops slots that a search still needs, and the search then
//! answers a menu confidently and differently.
//!
//! AND WHAT THE CODE MAKES OF THEM, which no content hash can see. The facts are the dialogue's,
//! but the code decides how they are written down: an inert slot is stored by NUMBER, and the
//! numbers come from which variables the graph counts a node as reading. A change there - a
//! modifier's guard starting to count as a read, say - renumbers the slots under a file that
//! still matches its dialogue, and the search then treats a live slot as inert and answers a
//! menu confidently and wrongly. So a stored answer also carries [`DERIVATION`], and
//! `tests/kept_facts.rs` pins a fingerprint of the facts beside it, so a change to what they
//! hold fails a test until the number is bumped.
//!
//! NO HASHES, NO CACHE. An index with no header carries no hashes at all; it is the mod
//! shipping a build intermediate, which is allowed. Nothing can be validated against it, so
//! nothing is stored or read, and every group is worked out as it was before this existed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::graph::LookAheadGraph;
use crate::index::{FORMAT_VERSION, Index};

/// How the facts are derived, bumped by hand whenever the code changes what a group's facts
/// come out as - see the module documentation. `tests/kept_facts.rs` says when.
pub const DERIVATION: u32 = 2;

/// Everything a group implies that this module keeps.
///
/// ONE STRUCT RATHER THAN A FILE PER FACT, so a second fact costs a field rather than another
/// read: the cost of a kept answer is mostly the open, not the bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupFacts {
    /// See [`LookAheadGraph::inert_slots`]. Sorted, so the file is stable across runs.
    pub inert_slots: Vec<usize>,
    /// See [`LookAheadGraph::settled_candidates`], which is a dominator tree over the whole
    /// group and the more expensive of the two.
    #[serde(default)]
    pub settled: crate::graph::settled::Candidates,
}

/// A stored answer, with the key it was worked out from.
#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    /// The index format the answer was worked out against.
    format: i32,
    /// The [`DERIVATION`] that worked it out.
    derivation: u32,
    /// What the group's conversations reduced to when it was stored - see [`content_of`].
    content: String,
    facts: GroupFacts,
}

/// What a group's conversations reduce to: every member and its content hash, in id order.
///
/// Empty where the index cannot say - a conversation it does not hold, or one with no hash,
/// which is the full index that carries none. A caller reads that as "do not keep this".
///
/// ONE ROUTINE, because a key written twice is a key that drifts, and the two writers here are
/// the engine and the measurements - which have to agree or a measurement exercises a cache the
/// product does not.
pub fn content_of(index: &Index, group: &[i32]) -> String {
    let mut ordered: Vec<i32> = group.to_vec();
    ordered.sort_unstable();
    let mut key = String::new();
    for conversation in ordered {
        let Some(record) = index.get(&conversation) else {
            return String::new();
        };
        if record.hash.is_empty() {
            return String::new();
        }
        key.push_str(&format!("{conversation}:{}\u{1}", record.hash));
    }
    key
}

/// Where a group's facts are kept.
#[derive(Debug, Clone)]
pub struct FactStore {
    root: PathBuf,
}

impl FactStore {
    /// The store beside `index`, which is where derived data belongs: it is worthless without
    /// the index it came from, and a new index should be able to take it away with one
    /// directory.
    pub fn beside(index: &Path) -> Self {
        let root = index
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("group-facts");
        Self { root }
    }

    /// Gives `graph` the facts for its group, reading them where a valid answer is stored and
    /// working them out and storing them where one is not.
    ///
    /// NOTHING HERE IS AN ERROR A CALLER HEARS ABOUT. A store that cannot be read or written -
    /// an install directory nobody may write to, a file half-written by a process that died -
    /// costs the time this was meant to save and nothing else, because the answer is always
    /// available from the graph. Refusing to answer a menu because a cache would not open
    /// would be a far worse failure than the one being avoided.
    /// `content` is what [`content_of`] made of the group, and an empty one keeps nothing.
    ///
    /// TAKEN RATHER THAN DERIVED HERE, because a caller that has it must not be made to read
    /// the index to hand it over: a measurement whose graph is already kept never reads the
    /// index at all, and `tests/kept_cache.rs` fails if anything makes it - see the note on
    /// `prepared::Prepared::content`, which is this key taken where the index was already open.
    pub fn fill(&self, graph: &LookAheadGraph, group: &[i32], content: &str) {
        if content.is_empty() {
            return;
        }
        let at = self.at(group);

        if let Some(stored) = Self::read(&at)
            && stored.format == FORMAT_VERSION
            && stored.derivation == DERIVATION
            && stored.content == content
        {
            graph.remember_inert_slots(stored.facts.inert_slots.into_iter().collect());
            graph.remember_settled_candidates(stored.facts.settled);
            return;
        }

        // THE MISS PAYS FOR THE HIT. Asking the graph is what costs the 14 ms this exists to
        // avoid, and it is asked here rather than left to whoever asks next so that the answer
        // is on disk before this process ends.
        let mut inert_slots: Vec<usize> = graph.inert_slots().iter().copied().collect();
        inert_slots.sort_unstable();
        let _ = Self::write(
            &at,
            &Stored {
                format: FORMAT_VERSION,
                derivation: DERIVATION,
                content: content.to_string(),
                facts: GroupFacts {
                    inert_slots,
                    settled: graph.settled_candidates().clone(),
                },
            },
        );
    }

    /// The file a group's answer lives in.
    ///
    /// NAMED BY THE LOWEST CONVERSATION IN THE GROUP, which is a name every member agrees on -
    /// a group is discovered whole from any of them, so naming it by the one that was asked
    /// for would store the same answer eleven times. A name that collided anyway would be
    /// caught by the key inside the file rather than believed.
    fn at(&self, group: &[i32]) -> PathBuf {
        let named = group.iter().min().copied().unwrap_or_default();
        self.root.join(format!("{named}.bin"))
    }

    fn read(at: &Path) -> Option<Stored> {
        bincode::deserialize(&std::fs::read(at).ok()?).ok()
    }

    fn write(at: &Path, stored: &Stored) -> std::io::Result<()> {
        if let Some(root) = at.parent() {
            std::fs::create_dir_all(root)?;
        }
        let encoded = bincode::serialize(stored)
            .map_err(|problem| std::io::Error::new(std::io::ErrorKind::InvalidData, problem))?;
        std::fs::write(at, encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::ConversationRecord;

    /// An index of one conversation with the hash it is given.
    fn index_holding(conversation: i32, hash: &str) -> Index {
        let mut index = Index::new();
        index.insert(
            conversation,
            ConversationRecord {
                id: conversation,
                hash: hash.to_string(),
                fields: Default::default(),
                entries: Vec::new(),
            },
        );
        index
    }

    fn store_at(named: &str) -> (FactStore, PathBuf) {
        let root = std::env::temp_dir().join(named);
        let _ = std::fs::remove_dir_all(&root);
        (FactStore { root: root.clone() }, root)
    }

    fn stored_for(root: &Path, group: &[i32]) -> Option<Stored> {
        FactStore::read(
            &FactStore {
                root: root.to_path_buf(),
            }
            .at(group),
        )
    }

    /// A miss writes the answer, and what it wrote carries the key it was worked out from.
    #[test]
    fn an_answer_survives_the_process_that_worked_it_out() {
        let (store, root) = store_at("degct-facts-survive");
        let index = index_holding(1, "a-hash");
        let graph = crate::graph::LookAheadGraph::new(vec![], Default::default()).unwrap();

        store.fill(&graph, &[1], &content_of(&index, &[1]));

        let stored = stored_for(&root, &[1]).expect("the answer was written");
        assert_eq!(stored.content, content_of(&index, &[1]));
        assert_eq!(stored.format, FORMAT_VERSION);
        assert_eq!(stored.derivation, DERIVATION);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A conversation whose content moved invalidates the answer, and it is worked out again.
    ///
    /// THE FAILURE THIS PREVENTS is the only one that matters here: a stale answer names slots
    /// by numbers the new index does not give them, so a search would drop slots it still
    /// needs and answer a menu differently with nothing to say it had.
    #[test]
    fn an_answer_is_refused_once_its_conversation_changes() {
        let (store, root) = store_at("degct-facts-refused");
        let graph = crate::graph::LookAheadGraph::new(vec![], Default::default()).unwrap();
        let before = index_holding(1, "before");
        let after = index_holding(1, "after");

        store.fill(&graph, &[1], &content_of(&before, &[1]));
        store.fill(&graph, &[1], &content_of(&after, &[1]));

        assert_eq!(
            stored_for(&root, &[1]).expect("rewritten").content,
            content_of(&after, &[1]),
            "a stored answer whose conversation changed was kept"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An index with no hashes is a build intermediate, and nothing is stored against it.
    #[test]
    fn an_index_that_cannot_be_validated_stores_nothing() {
        let (store, root) = store_at("degct-facts-unhashed");
        let graph = crate::graph::LookAheadGraph::new(vec![], Default::default()).unwrap();

        store.fill(&graph, &[1], &content_of(&index_holding(1, ""), &[1]));

        assert!(stored_for(&root, &[1]).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A group is keyed on ALL of its conversations, so one of them changing is enough.
    ///
    /// The case this is about: 640's group is eleven conversations, and an answer keyed on the
    /// one that was asked for would survive a change to any of the other ten.
    #[test]
    fn a_group_is_keyed_on_every_conversation_in_it() {
        let mut index = index_holding(1, "first");
        index.insert(
            2,
            ConversationRecord {
                id: 2,
                hash: "second".to_string(),
                fields: Default::default(),
                entries: Vec::new(),
            },
        );
        let both = content_of(&index, &[1, 2]);

        index.get_mut(&2).expect("it is there").hash = "moved".to_string();

        assert_ne!(both, content_of(&index, &[1, 2]));
        assert_eq!(both, format!("1:first\u{1}2:second\u{1}"));
    }

    /// A stored answer is TAKEN rather than worked out again, which is the whole point of
    /// keeping one.
    ///
    /// THE ANSWER PUT ON DISK IS ONE THE GRAPH COULD NOT REACH BY ITSELF - an empty graph has
    /// no slots at all, so a slot 7 that comes back came off the disk. A test that stored the
    /// right answer would pass whether or not the file was ever opened.
    #[test]
    fn a_stored_answer_is_the_one_the_graph_uses() {
        let (store, root) = store_at("degct-facts-taken");
        let index = index_holding(1, "a-hash");
        let content = content_of(&index, &[1]);
        let graph = crate::graph::LookAheadGraph::new(vec![], Default::default()).unwrap();

        let invented = invented_facts();
        write_invented(&store, &content, DERIVATION, &invented);

        store.fill(&graph, &[1], &content);

        assert_eq!(graph.settled_candidates(), &invented.settled);
        assert!(graph.inert_slots().contains(&3));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An answer another derivation wrote is refused, however well its dialogue still matches.
    ///
    /// THE CASE THIS IS ABOUT: a change to which variables a node counts as reading renumbers
    /// the slots, and a file written before it names live slots inert. Its conversations have
    /// not moved, so only the derivation can tell it apart - and a search that takes it drops
    /// a star the dialogue has.
    #[test]
    fn an_answer_from_another_derivation_is_refused() {
        let (store, root) = store_at("degct-facts-derivation");
        let index = index_holding(1, "a-hash");
        let content = content_of(&index, &[1]);
        let graph = crate::graph::LookAheadGraph::new(vec![], Default::default()).unwrap();

        let invented = invented_facts();
        write_invented(&store, &content, DERIVATION - 1, &invented);

        store.fill(&graph, &[1], &content);

        assert!(graph.settled_candidates().is_empty());
        assert!(!graph.inert_slots().contains(&3));
        assert_eq!(
            stored_for(&root, &[1]).expect("rewritten").derivation,
            DERIVATION
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Facts no empty graph could work out for itself, so one that comes back came off disk.
    fn invented_facts() -> GroupFacts {
        GroupFacts {
            inert_slots: vec![3],
            settled: vec![crate::graph::settled::Candidate {
                slot: 7,
                written_at: Vec::new(),
            }],
        }
    }

    /// Stores `facts` for group 1 as if `derivation` had worked them out.
    fn write_invented(store: &FactStore, content: &str, derivation: u32, facts: &GroupFacts) {
        FactStore::write(
            &store.at(&[1]),
            &Stored {
                format: FORMAT_VERSION,
                derivation,
                content: content.to_string(),
                facts: facts.clone(),
            },
        )
        .expect("the answer writes");
    }
}
