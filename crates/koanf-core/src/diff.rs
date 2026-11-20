//! Snapshot change sets.

use std::fmt;
use std::sync::Arc;

use crate::path::Path;
use crate::redact::redact_at;
use crate::value::Value;

/// A single change between two configuration snapshots.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Path of the changed value.
    pub path: Path,
    /// Kind of change.
    pub kind: ChangeKind,
}

/// The kind of a [`Change`].
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeKind {
    /// A key or subtree was added.
    Added(Value),
    /// A key or subtree was removed.
    Removed(Value),
    /// A value changed. Arrays are reported as a whole.
    Modified {
        /// Previous value.
        old: Value,
        /// New value.
        new: Value,
    },
}

/// An immutable, cheaply cloneable set of changes.
///
/// `Debug` output redacts values at paths configured through
/// [`ConfigBuilder::redact`](crate::ConfigBuilder::redact). Direct access
/// through [`Change`] and [`ChangeKind`] returns the original values.
#[derive(Clone)]
pub struct ChangeSet {
    changes: Arc<[Change]>,
    redactions: Arc<[Path]>,
}

impl ChangeSet {
    /// Creates a change set from a vector of changes.
    pub fn new(changes: Vec<Change>) -> Self {
        Self {
            changes: changes.into(),
            redactions: Arc::from(Vec::new()),
        }
    }

    /// Creates an empty change set.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns `true` if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Returns the number of changes.
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Iterates over the changes.
    pub fn iter(&self) -> std::slice::Iter<'_, Change> {
        self.changes.iter()
    }

    /// Returns the changes as a slice.
    pub fn as_slice(&self) -> &[Change] {
        &self.changes
    }

    pub(crate) fn diff_redacted(old: &Value, new: &Value, redactions: &[Path]) -> Self {
        let mut changes = Vec::new();
        diff_value(old, new, &Path::root(), &mut changes);
        Self {
            changes: changes.into(),
            redactions: Arc::from(redactions),
        }
    }
}

impl Default for ChangeSet {
    fn default() -> Self {
        Self {
            changes: Arc::from(Vec::new()),
            redactions: Arc::from(Vec::new()),
        }
    }
}

impl PartialEq for ChangeSet {
    fn eq(&self, other: &Self) -> bool {
        self.changes == other.changes
    }
}

impl fmt::Debug for ChangeSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.changes.iter().map(|change| RedactedChange {
                change,
                redactions: &self.redactions,
            }))
            .finish()
    }
}

struct RedactedChange<'a> {
    change: &'a Change,
    redactions: &'a [Path],
}

impl fmt::Debug for RedactedChange<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = &self.change.path;
        let kind = match &self.change.kind {
            ChangeKind::Added(value) => ChangeKind::Added(redact_at(value, path, self.redactions)),
            ChangeKind::Removed(value) => {
                ChangeKind::Removed(redact_at(value, path, self.redactions))
            }
            ChangeKind::Modified { old, new } => ChangeKind::Modified {
                old: redact_at(old, path, self.redactions),
                new: redact_at(new, path, self.redactions),
            },
        };
        formatter
            .debug_struct("Change")
            .field("path", path)
            .field("kind", &kind)
            .finish()
    }
}

impl<'a> IntoIterator for &'a ChangeSet {
    type Item = &'a Change;
    type IntoIter = std::slice::Iter<'a, Change>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

fn diff_value(old: &Value, new: &Value, path: &Path, changes: &mut Vec<Change>) {
    if old == new {
        return;
    }

    match (old, new) {
        (Value::Map(old_map), Value::Map(new_map)) => {
            for (key, old_value) in old_map {
                let child_path = path.child(key);
                match new_map.get(key) {
                    Some(new_value) => diff_value(old_value, new_value, &child_path, changes),
                    None => changes.push(Change {
                        path: child_path,
                        kind: ChangeKind::Removed(old_value.clone()),
                    }),
                }
            }
            for (key, new_value) in new_map {
                if !old_map.contains_key(key) {
                    changes.push(Change {
                        path: path.child(key),
                        kind: ChangeKind::Added(new_value.clone()),
                    });
                }
            }
        }
        (Value::Array(_), Value::Array(_)) | (_, _) => changes.push(Change {
            path: path.clone(),
            kind: ChangeKind::Modified {
                old: old.clone(),
                new: new.clone(),
            },
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Map;

    fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect::<Map>(),
        )
    }

    #[test]
    fn reports_added_removed_and_modified() {
        let old = map([
            ("same", Value::from(1)),
            ("changed", Value::from(1)),
            ("removed", Value::from(1)),
        ]);
        let new = map([
            ("same", Value::from(1)),
            ("changed", Value::from(2)),
            ("added", Value::from(1)),
        ]);

        let changes = ChangeSet::diff_redacted(&old, &new, &[]);
        assert_eq!(changes.len(), 3);

        let kinds = changes
            .iter()
            .map(|change| (&change.path, &change.kind))
            .collect::<Vec<_>>();
        assert!(kinds.iter().any(|(path, kind)| {
            path.to_string() == "removed" && matches!(kind, ChangeKind::Removed(_))
        }));
        assert!(kinds.iter().any(|(path, kind)| {
            path.to_string() == "added" && matches!(kind, ChangeKind::Added(_))
        }));
        assert!(kinds.iter().any(|(path, kind)| {
            path.to_string() == "changed" && matches!(kind, ChangeKind::Modified { .. })
        }));
    }

    #[test]
    fn arrays_are_reported_as_a_whole() {
        let old = map([("items", Value::Array(vec![Value::from(1)]))]);
        let new = map([("items", Value::Array(vec![Value::from(1), Value::from(2)]))]);
        let changes = ChangeSet::diff_redacted(&old, &new, &[]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.as_slice()[0].path.to_string(), "items");
    }

    #[test]
    fn equal_values_produce_no_changes() {
        let value = map([("a", Value::from(1))]);
        assert!(ChangeSet::diff_redacted(&value, &value, &[]).is_empty());
    }

    #[test]
    fn numeric_and_nan_equality_is_stable_for_diffs() {
        let old = map([("uint", Value::Uint(1)), ("nan", Value::Float(f64::NAN))]);
        let new = map([("uint", Value::Int(1)), ("nan", Value::Float(f64::NAN))]);
        assert!(ChangeSet::diff_redacted(&old, &new, &[]).is_empty());
    }
}
