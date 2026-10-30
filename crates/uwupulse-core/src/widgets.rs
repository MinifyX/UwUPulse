//! Overlay widgets. The core keeps them as opaque JSON: the editor and the
//! overlay page agree on `WidgetConfig`, the core only checks what it relies
//! on itself (`id` names the URL, `name` the list) and the size.

use serde_json::{json, Map, Value};

/// Largest widget, serialised.
pub const MAX_WIDGET_BYTES: usize = 64 * 1024;
/// More widgets than anyone sets up; a cap so the file stays small.
pub const MAX_WIDGETS: usize = 100;
/// Custom CSS within a widget.
pub const MAX_CUSTOM_CSS_BYTES: usize = 32 * 1024;

/// The widget a fresh install starts with.
pub fn default_widget() -> Value {
    json!({
        "id": "classic",
        "name": "Classic",
        "preset": "classic",
        "color": "#ff4d6d",
        "textColor": "#ffffff",
        "background": "transparent",
        "font": "Inter, system-ui, sans-serif",
        "fontSize": 96,
        "showIcon": true,
        "showLabel": true,
        "animateBeat": true,
        "showGraph": false,
        "graphSeconds": 60,
        "zones": {
            "enabled": false,
            "items": [
                { "min": 0, "color": "#4dabf7" },
                { "min": 120, "color": "#ffd43b" },
                { "min": 150, "color": "#ff6b6b" }
            ]
        },
        "idleText": "--",
        "hideWhenIdle": false,
        "customCss": ""
    })
}

/// A widget ID goes into a URL path: 1–64 of `A-Z a-z 0-9 - _`.
pub fn valid_widget_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Checks a widget and gives it an ID if it has none yet.
pub fn normalize(widget: Value) -> Result<Value, String> {
    let mut obj: Map<String, Value> = match widget {
        Value::Object(o) => o,
        _ => return Err("a widget is a JSON object".into()),
    };
    match obj.get("id") {
        None => {
            obj.insert("id".into(), uuid::Uuid::new_v4().to_string().into());
        }
        Some(Value::String(s)) if s.is_empty() => {
            obj.insert("id".into(), uuid::Uuid::new_v4().to_string().into());
        }
        Some(Value::String(s)) if valid_widget_id(s) => {}
        _ => return Err("widget id must be 1–64 letters, digits, - or _".into()),
    }
    match obj.get("name") {
        Some(Value::String(s)) if s.chars().count() <= 64 => {}
        _ => return Err("widget name must be a string of at most 64 characters".into()),
    }
    match obj.get("customCss") {
        None | Some(Value::Null) => {}
        Some(Value::String(css)) if css.len() <= MAX_CUSTOM_CSS_BYTES => {}
        Some(Value::String(_)) => return Err("custom CSS is larger than 32 KiB".into()),
        Some(_) => return Err("customCss must be a string".into()),
    }
    let value = Value::Object(obj);
    let size = serde_json::to_vec(&value)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if size > MAX_WIDGET_BYTES {
        return Err("widget is larger than 64 KiB".into());
    }
    Ok(value)
}

pub fn id_of(widget: &Value) -> &str {
    widget.get("id").and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        assert!(normalize(default_widget()).is_ok());
        let fresh = normalize(json!({"name": "New"})).unwrap();
        assert!(valid_widget_id(id_of(&fresh)));
        assert!(normalize(json!({"id": "../x", "name": "a"})).is_err());
        assert!(normalize(json!({"id": "a", "name": 5})).is_err());
        assert!(normalize(json!([1])).is_err());
        let css =
            json!({"id": "a", "name": "b", "customCss": "x".repeat(MAX_CUSTOM_CSS_BYTES + 1)});
        assert!(normalize(css).is_err());
        let big = json!({"id": "a", "name": "b", "other": "x".repeat(MAX_WIDGET_BYTES)});
        assert!(normalize(big).is_err());
    }
}
