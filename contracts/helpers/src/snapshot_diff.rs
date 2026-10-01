use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Represents a single change identified during storage snapshot comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<K, V> {
    /// An entry was added to the storage snapshot.
    Added { key: K, value: V },
    /// An existing entry had its value modified.
    Modified { key: K, old_value: V, new_value: V },
    /// An entry was removed from the storage snapshot.
    Removed { key: K, value: V },
}

impl<K, V> Change<K, V> {
    /// Return a reference to the key associated with this change.
    pub fn key(&self) -> &K {
        match self {
            Change::Added { key, .. } => key,
            Change::Modified { key, .. } => key,
            Change::Removed { key, .. } => key,
        }
    }
}

/// Diff two storage snapshots represented as lists of key-value pairs.
///
/// This helper performs an efficient comparison between `before` and `after` snapshots.
/// It identifies entries that have been added, removed, or modified, and excludes
/// entries that are identical.
///
/// # Parameters
/// - `before`: The slice of key-value pairs representing the state of storage before.
/// - `after`: The slice of key-value pairs representing the state of storage after.
///
/// # Ordering Guarantees
/// The resulting change list is guaranteed to be sorted in a stable, deterministic order based
/// on the `Ord` implementation of the key. This ensures consistent diff reports regardless of
/// the input ordering of elements.
///
/// # Efficiency
/// The snapshots are loaded into `BTreeMap` structures in $O(N \log N + M \log M)$ time,
/// and then compared in a single linear $O(N + M)$ pass. The final list of changes is
/// sorted in $O(C \log C)$ where $C$ is the number of changes.
pub fn diff_snapshots<K, V>(before: &[(K, V)], after: &[(K, V)]) -> Vec<Change<K, V>>
where
    K: Ord + Clone,
    V: PartialEq + Clone,
{
    let mut before_map = BTreeMap::new();
    for (k, v) in before {
        before_map.insert(k.clone(), v.clone());
    }

    let mut after_map = BTreeMap::new();
    for (k, v) in after {
        after_map.insert(k.clone(), v.clone());
    }

    let mut changes = Vec::new();
    let mut before_iter = before_map.iter();
    let mut after_iter = after_map.iter();

    let mut current_before = before_iter.next();
    let mut current_after = after_iter.next();

    while let (Some((bk, bv)), Some((ak, av))) = (current_before, current_after) {
        if bk < ak {
            changes.push(Change::Removed {
                key: bk.clone(),
                value: bv.clone(),
            });
            current_before = before_iter.next();
        } else if bk > ak {
            changes.push(Change::Added {
                key: ak.clone(),
                value: av.clone(),
            });
            current_after = after_iter.next();
        } else {
            if bv != av {
                changes.push(Change::Modified {
                    key: bk.clone(),
                    old_value: bv.clone(),
                    new_value: av.clone(),
                });
            }
            current_before = before_iter.next();
            current_after = after_iter.next();
        }
    }

    while let Some((bk, bv)) = current_before {
        changes.push(Change::Removed {
            key: bk.clone(),
            value: bv.clone(),
        });
        current_before = before_iter.next();
    }

    while let Some((ak, av)) = current_after {
        changes.push(Change::Added {
            key: ak.clone(),
            value: av.clone(),
        });
        current_after = after_iter.next();
    }

    // Sort to guarantee stable, deterministic ordering.
    changes.sort_by(|a, b| a.key().cmp(b.key()));
    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn test_identical_snapshots() {
        let before = vec![("key1".to_string(), "val1".to_string())];
        let after = vec![("key1".to_string(), "val1".to_string())];
        let diff = diff_snapshots(&before, &after);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_added_keys() {
        let before = vec![];
        let after = vec![("key1".to_string(), "val1".to_string())];
        let diff = diff_snapshots(&before, &after);
        assert_eq!(
            diff,
            vec![Change::Added {
                key: "key1".to_string(),
                value: "val1".to_string(),
            }]
        );
    }

    #[test]
    fn test_removed_keys() {
        let before = vec![("key1".to_string(), "val1".to_string())];
        let after = vec![];
        let diff = diff_snapshots(&before, &after);
        assert_eq!(
            diff,
            vec![Change::Removed {
                key: "key1".to_string(),
                value: "val1".to_string(),
            }]
        );
    }

    #[test]
    fn test_modified_values() {
        let before = vec![("key1".to_string(), "val1".to_string())];
        let after = vec![("key1".to_string(), "val2".to_string())];
        let diff = diff_snapshots(&before, &after);
        assert_eq!(
            diff,
            vec![Change::Modified {
                key: "key1".to_string(),
                old_value: "val1".to_string(),
                new_value: "val2".to_string(),
            }]
        );
    }

    #[test]
    fn test_multiple_changes() {
        let before = vec![
            ("key1".to_string(), "val1".to_string()),
            ("key2".to_string(), "val2".to_string()),
        ];
        let after = vec![
            ("key2".to_string(), "val2_mod".to_string()),
            ("key3".to_string(), "val3".to_string()),
        ];
        let diff = diff_snapshots(&before, &after);
        assert_eq!(
            diff,
            vec![
                Change::Removed {
                    key: "key1".to_string(),
                    value: "val1".to_string(),
                },
                Change::Modified {
                    key: "key2".to_string(),
                    old_value: "val2".to_string(),
                    new_value: "val2_mod".to_string(),
                },
                Change::Added {
                    key: "key3".to_string(),
                    value: "val3".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_empty_snapshots() {
        let before: Vec<(String, String)> = vec![];
        let after: Vec<(String, String)> = vec![];
        let diff = diff_snapshots(&before, &after);
        assert!(diff.is_empty());
    }

    #[test]
    fn test_deterministic_ordering() {
        // Different input order should yield identical output order
        let before1 = vec![
            ("key2".to_string(), "val2".to_string()),
            ("key1".to_string(), "val1".to_string()),
        ];
        let after1 = vec![
            ("key3".to_string(), "val3".to_string()),
            ("key1".to_string(), "val1_mod".to_string()),
        ];

        let before2 = vec![
            ("key1".to_string(), "val1".to_string()),
            ("key2".to_string(), "val2".to_string()),
        ];
        let after2 = vec![
            ("key1".to_string(), "val1_mod".to_string()),
            ("key3".to_string(), "val3".to_string()),
        ];

        let diff1 = diff_snapshots(&before1, &after1);
        let diff2 = diff_snapshots(&before2, &after2);

        assert_eq!(diff1, diff2);
        assert_eq!(
            diff1,
            vec![
                Change::Modified {
                    key: "key1".to_string(),
                    old_value: "val1".to_string(),
                    new_value: "val1_mod".to_string(),
                },
                Change::Removed {
                    key: "key2".to_string(),
                    value: "val2".to_string(),
                },
                Change::Added {
                    key: "key3".to_string(),
                    value: "val3".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_realistic_fixtures() {
        // Simulated contract storage keys (represented as serialized hex strings/symbols)
        let before = vec![
            ("admin".to_string(), "GBBD47...".to_string()),
            ("balance".to_string(), "1000".to_string()),
            ("paused".to_string(), "false".to_string()),
        ];
        let after = vec![
            ("admin".to_string(), "GBBD47...".to_string()),
            ("balance".to_string(), "1500".to_string()), // modified
            ("paused".to_string(), "true".to_string()),  // modified
            ("pending_admin".to_string(), "GCCCCC...".to_string()), // added
        ];

        let diff = diff_snapshots(&before, &after);
        assert_eq!(
            diff,
            vec![
                Change::Modified {
                    key: "balance".to_string(),
                    old_value: "1000".to_string(),
                    new_value: "1500".to_string(),
                },
                Change::Modified {
                    key: "paused".to_string(),
                    old_value: "false".to_string(),
                    new_value: "true".to_string(),
                },
                Change::Added {
                    key: "pending_admin".to_string(),
                    value: "GCCCCC...".to_string(),
                },
            ]
        );
    }
}
