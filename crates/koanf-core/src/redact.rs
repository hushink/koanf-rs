//! Best-effort value redaction for debug output.

use crate::path::Path;
use crate::value::Value;

const REDACTED: &str = "***";

pub(crate) fn redact(value: &Value, redactions: &[Path]) -> Value {
    redact_at(value, &Path::root(), redactions)
}

pub(crate) fn redact_at(value: &Value, path: &Path, redactions: &[Path]) -> Value {
    if is_redacted(path, redactions) {
        return Value::String(REDACTED.to_owned());
    }
    match value {
        Value::Map(map) => Value::Map(
            map.iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        redact_at(value, &path.child(key.clone()), redactions),
                    )
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(index, value)| redact_at(value, &path.child(index.to_string()), redactions))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn is_redacted(path: &Path, redactions: &[Path]) -> bool {
    let current = path.segments();
    redactions.iter().any(|redaction| {
        let target = redaction.segments();
        current.len() >= target.len() && current[..target.len()] == *target
    })
}
