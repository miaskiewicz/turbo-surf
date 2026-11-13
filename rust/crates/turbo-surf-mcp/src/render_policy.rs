use crate::Session;
use serde_json::{json, Value};

pub(crate) fn set_mode(session: &mut Session, args: &Value) -> Result<Value, String> {
    let selected: Option<turbo_surf_render::AnimationPolicy> = args
        .get("animations")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()
        .map_err(|error| format!("Invalid animation presentation policy: {error}"))?;
    if let Some(policy) = selected {
        session.browsing_storage.set_animation_policy(policy)?;
    }
    session.mode = crate::arg_str(args, "mode").unwrap_or("no-js").to_string();
    // Mode-only calls retain their original protocol result shape.
    let mut result = json!({"mode": session.mode});
    if let Some(policy) = selected {
        result["animations"] = json!(policy);
    }
    Ok(result)
}
