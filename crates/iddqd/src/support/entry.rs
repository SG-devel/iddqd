#![allow(dead_code)]

//! Crate-internal support for classifying multi-key entry lookups.
//!
//! This module is intentionally independent of the map implementations. It only
//! understands fixed arrays of optional item indexes, preserving enough state for
//! entry APIs to reason about vacant, unique, and non-unique lookup results.

/// Classification of a multi-key entry lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryLookup<I, const N: usize> {
    /// No key matched an existing item.
    Vacant,
    /// Every key matched the same existing item.
    Unique(I),
    /// At least one key matched, but the lookup was not unique.
    NonUnique(NonUniqueIndexes<I, N>),
}

/// Per-key lookup indexes for an entry operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EntryIndexes<I, const N: usize> {
    indexes: [Option<I>; N],
}

/// Non-unique per-key lookup indexes.
///
/// Invariant: at least one index is `Some`, and the indexes are not all the
/// same `Some` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NonUniqueIndexes<I, const N: usize> {
    indexes: [Option<I>; N],
}

/// Distinct indexes referenced by a non-vacant lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DistinctIndexes<I, const N: usize> {
    indexes: [Option<I>; N],
    len: usize,
    key_to_slot: [Option<usize>; N],
}

impl<I: Copy + Eq, const N: usize> EntryIndexes<I, N> {
    #[inline]
    pub(crate) const fn new(indexes: [Option<I>; N]) -> Self {
        Self { indexes }
    }

    #[inline]
    pub(crate) const fn indexes(&self) -> &[Option<I>; N] {
        &self.indexes
    }

    #[inline]
    pub(crate) fn classify(self) -> EntryLookup<I, N> {
        let mut first = None;
        let mut saw_none = false;
        let mut all_some_same = true;

        for index in self.indexes {
            match (first, index) {
                (None, Some(index)) => first = Some(index),
                (Some(first_index), Some(index)) if first_index != index => {
                    all_some_same = false;
                }
                (_, None) => saw_none = true,
                _ => {}
            }
        }

        match (first, saw_none, all_some_same) {
            (None, _, _) => EntryLookup::Vacant,
            (Some(index), false, true) => EntryLookup::Unique(index),
            (Some(_), _, _) => EntryLookup::NonUnique(NonUniqueIndexes {
                indexes: self.indexes,
            }),
        }
    }
}

impl<I: Copy + Eq, const N: usize> NonUniqueIndexes<I, N> {
    #[inline]
    pub(crate) const fn indexes(&self) -> &[Option<I>; N] {
        &self.indexes
    }

    #[inline]
    pub(crate) fn distinct(self) -> DistinctIndexes<I, N> {
        DistinctIndexes::from_indexes(self.indexes)
    }
}

impl<I: Copy + Eq, const N: usize> DistinctIndexes<I, N> {
    fn from_indexes(source: [Option<I>; N]) -> Self {
        let mut indexes = [None; N];
        let mut key_to_slot = [None; N];
        let mut len = 0;

        for (key, source_index) in source.into_iter().enumerate() {
            if let Some(source_index) = source_index {
                let mut slot = None;

                // Distinct indexes are stored densely in first-key-hit order.
                // Only the initialized prefix `..len` is inspected here.
                for (candidate_slot, candidate) in
                    indexes[..len].iter().enumerate()
                {
                    if *candidate == Some(source_index) {
                        slot = Some(candidate_slot);
                        break;
                    }
                }

                let slot = match slot {
                    Some(slot) => slot,
                    None => {
                        let slot = len;
                        indexes[slot] = Some(source_index);
                        len += 1;
                        slot
                    }
                };
                key_to_slot[key] = Some(slot);
            }
        }

        Self { indexes, len, key_to_slot }
    }

    #[inline]
    pub(crate) const fn indexes(&self) -> &[Option<I>; N] {
        &self.indexes
    }

    #[inline]
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub(crate) const fn key_to_slot(&self) -> &[Option<usize>; N] {
        &self.key_to_slot
    }
}

#[cfg(test)]
mod tests {
    use super::{EntryIndexes, EntryLookup};

    fn classify<const N: usize>(
        indexes: [Option<u8>; N],
    ) -> EntryLookup<u8, N> {
        EntryIndexes::new(indexes).classify()
    }

    fn non_unique_distinct<const N: usize>(
        indexes: [Option<u8>; N],
    ) -> (usize, [Option<u8>; N], [Option<usize>; N]) {
        let EntryLookup::NonUnique(indexes) = classify(indexes) else {
            panic!("expected non-unique indexes")
        };
        let distinct = indexes.distinct();
        (distinct.len(), *distinct.indexes(), *distinct.key_to_slot())
    }

    #[test]
    fn arity_2_vacant_classification() {
        assert_eq!(classify([None, None]), EntryLookup::Vacant);
    }

    #[test]
    fn arity_2_unique_classification() {
        assert_eq!(classify([Some(1), Some(1)]), EntryLookup::Unique(1));
    }

    #[test]
    fn arity_2_partial_classification() {
        assert!(matches!(classify([Some(1), None]), EntryLookup::NonUnique(_)));
        assert!(matches!(classify([None, Some(1)]), EntryLookup::NonUnique(_)));
    }

    #[test]
    fn arity_2_mixed_classification() {
        assert!(matches!(
            classify([Some(1), Some(2)]),
            EntryLookup::NonUnique(_)
        ));
    }

    #[test]
    fn arity_3_vacant_classification() {
        assert_eq!(classify([None, None, None]), EntryLookup::Vacant);
    }

    #[test]
    fn arity_3_unique_classification() {
        assert_eq!(
            classify([Some(1), Some(1), Some(1)]),
            EntryLookup::Unique(1)
        );
    }

    #[test]
    fn arity_3_partial_duplicate_classification() {
        assert!(matches!(
            classify([Some(1), Some(1), None]),
            EntryLookup::NonUnique(_)
        ));
        assert!(matches!(
            classify([None, Some(1), Some(1)]),
            EntryLookup::NonUnique(_)
        ));
    }

    #[test]
    fn arity_3_separated_duplicate_classification() {
        assert!(matches!(
            classify([Some(1), None, Some(1)]),
            EntryLookup::NonUnique(_)
        ));
    }

    #[test]
    fn arity_3_mixed_duplicate_classification() {
        assert!(matches!(
            classify([Some(1), Some(1), Some(2)]),
            EntryLookup::NonUnique(_)
        ));
        assert!(matches!(
            classify([Some(1), Some(2), Some(1)]),
            EntryLookup::NonUnique(_)
        ));
    }

    #[test]
    fn arity_3_all_distinct_classification() {
        assert!(matches!(
            classify([Some(1), Some(2), Some(3)]),
            EntryLookup::NonUnique(_)
        ));
    }

    #[test]
    fn deterministic_first_key_hit_distinct_ordering() {
        assert_eq!(
            non_unique_distinct([Some(1), Some(1), Some(2)]),
            (2, [Some(1), Some(2), None], [Some(0), Some(0), Some(1)])
        );
        assert_eq!(
            non_unique_distinct([Some(1), Some(2), Some(1)]),
            (2, [Some(1), Some(2), None], [Some(0), Some(1), Some(0)])
        );
        assert_eq!(
            non_unique_distinct([None, Some(2), Some(1)]),
            (2, [Some(2), Some(1), None], [None, Some(0), Some(1)])
        );
    }

    #[test]
    fn key_to_slot_mapping_for_repeated_indexes() {
        assert_eq!(
            non_unique_distinct([Some(1), None, Some(1)]),
            (1, [Some(1), None, None], [Some(0), None, Some(0)])
        );
    }

    #[test]
    fn no_duplicate_distinct_indexes() {
        assert_eq!(
            non_unique_distinct([Some(1), Some(2), Some(3)]),
            (3, [Some(1), Some(2), Some(3)], [Some(0), Some(1), Some(2)])
        );
    }
}
