use serde_json::Value;

/// Lists longer than this are truncated in default `--json` output; a sibling
/// `<field>_omitted` count says how many items were left out.
pub const DEFAULT_ITEM_CAP: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonMode {
    Off,
    Bounded,
    Full,
}

pub fn render<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

pub fn render_bounded<T: serde::Serialize + ?Sized>(
    value: &T,
    mode: JsonMode,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(value)?;
    if mode == JsonMode::Bounded {
        cap_lists(&mut value, DEFAULT_ITEM_CAP);
    }
    serde_json::to_string_pretty(&value)
}

/// Truncates every list held by an object field and records the omitted count beside
/// it, recursing into the items that remain.
pub fn cap_lists(value: &mut Value, cap: usize) {
    match value {
        Value::Object(map) => {
            let mut omitted = Vec::new();
            for (key, field) in map.iter_mut() {
                if let Value::Array(items) = field {
                    if items.len() > cap {
                        omitted.push((format!("{key}_omitted"), items.len() - cap));
                        items.truncate(cap);
                    }
                }
                cap_lists(field, cap);
            }
            for (key, count) in omitted {
                map.insert(key, Value::from(count));
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| cap_lists(item, cap)),
        _ => {}
    }
}
