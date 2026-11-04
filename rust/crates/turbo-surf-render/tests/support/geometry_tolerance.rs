//! Chrome 156 geometry comparison for the macOS default-font controls.
//!
//! turbo-dom 0.7.0 resolves the macOS HTML defaults like Chrome 156 (sans-serif ->
//! Helvetica, monospace -> Courier; turbo-dom#259/#260), but line heights and
//! advances still differ from Blink's by up to a few px. Every number may differ by
//! at most `TOLERANCE_PX`; the structure (keys, box/rect counts, nesting, order,
//! strings, booleans, nulls) must match exactly.
use serde_json::Value;

pub const TOLERANCE_PX: f64 = 4.0;

/// Every difference beyond the tolerance or in structure, as `path: native vs Chrome`.
pub fn geometry_differences(actual: &Value, expected: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk(actual, expected, "", &mut out);
    out
}

fn walk(actual: &Value, expected: &Value, path: &str, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) => {
            let (a, e) = (a.as_f64().unwrap(), e.as_f64().unwrap());
            if (a - e).abs() > TOLERANCE_PX {
                out.push(format!("{path}: {a} vs {e}"));
            }
        }
        (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
            for (index, (a, e)) in a.iter().zip(e).enumerate() {
                walk(a, e, &format!("{path}[{index}]"), out);
            }
        }
        (Value::Object(a), Value::Object(e))
            if a.len() == e.len() && a.keys().zip(e.keys()).all(|(a, e)| a == e) =>
        {
            for (key, e) in e {
                walk(&a[key], e, &format!("{path}.{key}"), out);
            }
        }
        _ if actual != expected => out.push(format!("{path}: {actual} vs {expected}")),
        _ => {}
    }
}
