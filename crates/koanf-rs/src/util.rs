use koanf_core::{Error, Map, Path, Result, Value};

pub(crate) fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

pub(crate) fn insert_path(map: &mut Map, path: &Path, value: Value, source: &str) -> Result<()> {
    insert_segments(map, path.segments(), value, source)
}

fn insert_segments(map: &mut Map, segments: &[String], value: Value, source: &str) -> Result<()> {
    let Some((first, rest)) = segments.split_first() else {
        return Err(Error::SourceConflict {
            source: source.to_owned(),
            message: "cannot insert an empty path".to_owned(),
        });
    };

    if rest.is_empty() {
        map.insert(first.clone(), value);
        return Ok(());
    }

    let entry = map
        .entry(first.clone())
        .or_insert_with(|| Value::Map(Map::new()));
    match entry {
        Value::Map(child) => insert_segments(child, rest, value, source),
        other => Err(Error::SourceConflict {
            source: source.to_owned(),
            message: format!(
                "`{first}` is a {} and cannot be traversed",
                other.type_name()
            ),
        }),
    }
}
