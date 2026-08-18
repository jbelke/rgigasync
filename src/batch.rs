//! Splitting a scan into bounded batches.
//!
//! Batching is what makes `rgigasync` different from a plain `rsync`: rsync
//! builds its entire file list in memory before transferring anything, so a
//! tree with tens of millions of entries can exhaust RAM long before a single
//! byte moves. Planning batches up front — rather than flushing a shared
//! buffer from inside the walk — keeps batch boundaries exact and makes the
//! whole thing deterministic and testable.

use crate::scan::Entry;

/// A bounded slice of the scan, transferred by a single rsync invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub entries: Vec<Entry>,
}

impl Batch {
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.entries
            .iter()
            .fold(0u64, |acc, e| acc.saturating_add(e.size))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Limits applied to every batch.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum bytes per batch. A single entry larger than this still gets its
    /// own batch — files are never split across rsync runs.
    pub max_bytes: u64,
    /// Maximum entries per batch; `0` means unlimited. This is the knob that
    /// actually bounds rsync's memory on trees of tiny files, where the byte
    /// limit alone would let a batch grow to millions of paths.
    pub max_entries: usize,
}

/// Group `entries` into batches that respect `limits`, preserving scan order.
///
/// Order is preserved so that directories — which `WalkDir` yields before
/// their contents — are created at the destination no later than the files
/// inside them.
#[must_use]
pub fn plan(entries: Vec<Entry>, limits: Limits) -> Vec<Batch> {
    let mut batches = Vec::new();
    let mut current: Vec<Entry> = Vec::new();
    let mut current_bytes: u64 = 0;

    for entry in entries {
        let would_exceed_bytes = current_bytes.saturating_add(entry.size) > limits.max_bytes;
        let would_exceed_count = limits.max_entries > 0 && current.len() + 1 > limits.max_entries;

        if !current.is_empty() && (would_exceed_bytes || would_exceed_count) {
            batches.push(Batch {
                entries: std::mem::take(&mut current),
            });
            current_bytes = 0;
        }

        current_bytes = current_bytes.saturating_add(entry.size);
        current.push(entry);
    }

    if !current.is_empty() {
        batches.push(Batch { entries: current });
    }

    batches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::EntryKind;
    use std::path::PathBuf;

    fn file(name: &str, size: u64) -> Entry {
        Entry {
            rel_path: PathBuf::from(name),
            size,
            kind: EntryKind::File,
        }
    }

    fn limits(max_bytes: u64, max_entries: usize) -> Limits {
        Limits {
            max_bytes,
            max_entries,
        }
    }

    fn shape(batches: &[Batch]) -> Vec<Vec<&str>> {
        batches
            .iter()
            .map(|b| {
                b.entries
                    .iter()
                    .map(|e| e.rel_path.to_str().unwrap())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn empty_input_yields_no_batches() {
        assert!(plan(Vec::new(), limits(100, 0)).is_empty());
    }

    #[test]
    fn fills_a_batch_up_to_the_byte_limit() {
        let batches = plan(
            vec![file("a", 40), file("b", 40), file("c", 40)],
            limits(100, 0),
        );
        assert_eq!(shape(&batches), vec![vec!["a", "b"], vec!["c"]]);
    }

    #[test]
    fn a_batch_exactly_at_the_limit_is_not_split() {
        let batches = plan(vec![file("a", 50), file("b", 50)], limits(100, 0));
        assert_eq!(shape(&batches), vec![vec!["a", "b"]]);
        assert_eq!(batches[0].bytes(), 100);
    }

    #[test]
    fn an_oversized_file_gets_its_own_batch_rather_than_being_split() {
        let batches = plan(
            vec![file("small", 10), file("huge", 5_000), file("tail", 10)],
            limits(100, 0),
        );
        assert_eq!(
            shape(&batches),
            vec![vec!["small"], vec!["huge"], vec!["tail"]]
        );
    }

    #[test]
    fn entry_count_limit_bounds_batches_of_zero_byte_entries() {
        // Directories and symlinks carry no bytes, so only the count limit can
        // stop a batch from growing without bound.
        let dirs: Vec<Entry> = (0..10)
            .map(|i| Entry {
                rel_path: PathBuf::from(format!("d{i}")),
                size: 0,
                kind: EntryKind::Dir,
            })
            .collect();
        let batches = plan(dirs, limits(u64::MAX, 4));
        assert_eq!(batches.len(), 3);
        assert_eq!(
            batches.iter().map(Batch::len).collect::<Vec<_>>(),
            vec![4, 4, 2]
        );
    }

    #[test]
    fn zero_entry_limit_means_unlimited() {
        let batches = plan(
            (0..100).map(|i| file(&i.to_string(), 0)).collect(),
            limits(u64::MAX, 0),
        );
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 100);
    }

    #[test]
    fn every_input_entry_appears_exactly_once_and_in_order() {
        let input: Vec<Entry> = (0..500).map(|i| file(&format!("f{i}"), i % 37)).collect();
        let batches = plan(input.clone(), limits(64, 7));

        let flattened: Vec<Entry> = batches.into_iter().flat_map(|b| b.entries).collect();
        assert_eq!(
            flattened, input,
            "batching must not drop, duplicate or reorder"
        );
    }

    #[test]
    fn a_running_total_that_would_overflow_still_splits() {
        // Wrapping addition would make `u64::MAX + 1` equal 0, which compares
        // as *under* the limit and would silently merge the two into one batch.
        let batches = plan(
            vec![file("huge", u64::MAX), file("tiny", 1)],
            limits(1_000, 0),
        );
        assert_eq!(shape(&batches), vec![vec!["huge"], vec!["tiny"]]);
    }

    #[test]
    fn byte_totals_do_not_panic_on_overflow() {
        let batch = Batch {
            entries: vec![file("a", u64::MAX), file("b", u64::MAX)],
        };
        assert_eq!(batch.bytes(), u64::MAX);
    }
}
