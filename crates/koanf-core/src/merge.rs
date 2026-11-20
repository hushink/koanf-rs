//! Layered merge semantics.

use crate::value::{Map, Value};

/// How arrays are combined when both the base and overlay value are arrays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ArrayMerge {
    /// The overlay array replaces the base array.
    #[default]
    Replace,
    /// The overlay array is appended to the base array.
    Append,
    /// The overlay array is prepended to the base array.
    Prepend,
}

/// How explicit `null` values in an overlay are handled.
///
/// Null handling applies to object values. Nulls inside arrays are always
/// treated as values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NullMerge {
    /// Store `null` in the merged object.
    #[default]
    Keep,
    /// Remove the key from the merged object.
    Delete,
    /// Keep the existing base value and ignore the null.
    Ignore,
}

/// Options controlling layered merging.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MergeOptions {
    /// Array merge behavior.
    pub arrays: ArrayMerge,
    /// Null merge behavior.
    pub nulls: NullMerge,
}

/// Merges `overlay` into `base` according to `options`.
///
/// Objects are merged recursively, scalars and arrays replace by default, and
/// existing keys keep their position. New keys are appended in overlay order.
pub fn merge_map(base: &mut Map, overlay: Map, options: &MergeOptions) {
    for (key, overlay_value) in overlay {
        if base.contains_key(&key) {
            let remove = {
                let base_value = base
                    .get_mut(&key)
                    .expect("contains_key was checked immediately before get_mut");
                match overlay_value {
                    Value::Null => match options.nulls {
                        NullMerge::Keep => {
                            *base_value = Value::Null;
                            false
                        }
                        NullMerge::Ignore => false,
                        NullMerge::Delete => true,
                    },
                    other => {
                        merge_value(base_value, other, options);
                        false
                    }
                }
            };
            if remove {
                base.shift_remove(&key);
            }
        } else if !(overlay_value.is_null() && options.nulls != NullMerge::Keep) {
            base.insert(key, overlay_value);
        }
    }
}

fn merge_value(base: &mut Value, overlay: Value, options: &MergeOptions) {
    match overlay {
        Value::Map(overlay_map) => match base {
            Value::Map(base_map) => merge_map(base_map, overlay_map, options),
            _ => *base = Value::Map(overlay_map),
        },
        Value::Array(overlay_array) => match base {
            Value::Array(base_array) => match options.arrays {
                ArrayMerge::Replace => *base_array = overlay_array,
                ArrayMerge::Append => base_array.extend(overlay_array),
                ArrayMerge::Prepend => {
                    let mut merged = overlay_array;
                    merged.append(base_array);
                    *base_array = merged;
                }
            },
            _ => *base = Value::Array(overlay_array),
        },
        other => *base = other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Map {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect()
    }

    #[test]
    fn deep_merges_objects_and_replaces_scalars() {
        let mut base = map([(
            "server",
            Value::Map(map([
                ("host", Value::from("localhost")),
                ("port", Value::from(80)),
            ])),
        )]);
        let overlay = map([(
            "server",
            Value::Map(map([
                ("port", Value::from(8080)),
                ("tls", Value::Bool(true)),
            ])),
        )]);

        merge_map(&mut base, overlay, &MergeOptions::default());

        let server = base["server"].as_map().unwrap();
        assert_eq!(server["host"], Value::from("localhost"));
        assert_eq!(server["port"], Value::from(8080));
        assert_eq!(server["tls"], Value::Bool(true));
        assert_eq!(
            server.keys().collect::<Vec<_>>(),
            vec!["host", "port", "tls"]
        );
    }

    #[test]
    fn replaces_arrays_by_default() {
        let mut base = map([("items", Value::Array(vec![Value::from(1)]))]);
        let overlay = map([("items", Value::Array(vec![Value::from(2)]))]);
        merge_map(&mut base, overlay, &MergeOptions::default());
        assert_eq!(base["items"], Value::Array(vec![Value::from(2)]));
    }

    #[test]
    fn appends_arrays_when_configured() {
        let mut base = map([("items", Value::Array(vec![Value::from(1)]))]);
        let overlay = map([("items", Value::Array(vec![Value::from(2)]))]);
        merge_map(
            &mut base,
            overlay,
            &MergeOptions {
                arrays: ArrayMerge::Append,
                ..MergeOptions::default()
            },
        );
        assert_eq!(
            base["items"],
            Value::Array(vec![Value::from(1), Value::from(2)])
        );
    }

    #[test]
    fn null_delete_removes_keys() {
        let mut base = map([("a", Value::from(1)), ("b", Value::from(2))]);
        let overlay = map([("a", Value::Null)]);
        merge_map(
            &mut base,
            overlay,
            &MergeOptions {
                nulls: NullMerge::Delete,
                ..MergeOptions::default()
            },
        );
        assert!(!base.contains_key("a"));
        assert_eq!(base["b"], Value::from(2));
    }

    #[test]
    fn null_ignore_keeps_existing_value() {
        let mut base = map([("a", Value::from(1))]);
        let overlay = map([("a", Value::Null)]);
        merge_map(
            &mut base,
            overlay,
            &MergeOptions {
                nulls: NullMerge::Ignore,
                ..MergeOptions::default()
            },
        );
        assert_eq!(base["a"], Value::from(1));
    }
}
