use super::*;

#[op2(reentrant)]
pub(super) fn op_dialog<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &mut OpState,
    window: v8::Local<'s, v8::Object>,
    #[string] kind: &str,
    message: v8::Local<'s, v8::String>,
    default_value: v8::Local<'s, v8::String>,
) -> Result<v8::Local<'s, v8::Value>, JsErrorBox> {
    let kind = match kind {
        "alert" => crate::DialogKind::Alert,
        "confirm" => crate::DialogKind::Confirm,
        "prompt" => crate::DialogKind::Prompt,
        _ => return Err(JsErrorBox::type_error("Unknown native dialog type")),
    };
    let context = window
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Dialog Window is unavailable"))?;
    if context.get_slot::<FrameRetired>().is_some() {
        return Err(JsErrorBox::generic("Dialog target closed"));
    }
    if context
        .get_slot::<FrameSandbox>()
        .is_some_and(|policy| policy.modals_blocked)
    {
        return Ok(match kind {
            crate::DialogKind::Confirm => v8::Boolean::new(scope, false).into(),
            _ => v8::null(scope).into(),
        });
    }
    let metadata = request_frame_metadata(scope, context, None);
    let target = auxiliary_pages::top_level_identity(scope, context).map(|id| id.0.to_string());
    let primary = target.as_ref().is_none_or(|target| {
        state
            .borrow::<auxiliary_pages::Pages>()
            .events
            .root()
            .as_ref()
            == Some(target)
    });
    let request = crate::DialogRequest {
        id: 0,
        owner: target.unwrap_or_else(|| "legacy-root".into()),
        primary,
        kind: kind.clone(),
        message: units(scope, message),
        default_value: units(scope, default_value),
    };
    let log = state.borrow::<NetworkLog>().clone();
    let reply = log
        .dialogs()
        .request(
            request,
            // The actor's existing watchdog owns this operation's exact budget.
            // No independent dialog timeout shortens or extends it.
            None::<std::time::Instant>,
            || scope.is_execution_terminating(),
            |request| {
                let kind = match request.kind {
                    crate::DialogKind::Alert => "alert",
                    crate::DialogKind::Confirm => "confirm",
                    crate::DialogKind::Prompt => "prompt",
                };
                log.push(
                    serde_json::json!({"event":"dialog","id":request.id.to_string(),
                "type":kind,"messageUnits":request.message,"defaultValueUnits":request.default_value,
                "context":{"frame":metadata}}),
                );
            },
        )
        .map_err(JsErrorBox::generic)?;
    Ok(match kind {
        crate::DialogKind::Alert => v8::null(scope).into(),
        crate::DialogKind::Confirm => v8::Boolean::new(scope, reply.accepted).into(),
        crate::DialogKind::Prompt if reply.accepted => match reply.prompt_text {
            Some(units) => v8::String::new_from_two_byte(scope, &units, v8::NewStringType::Normal)
                .ok_or_else(|| JsErrorBox::generic("Dialog reply string unavailable"))?
                .into(),
            None => default_value.into(),
        },
        crate::DialogKind::Prompt => v8::null(scope).into(),
    })
}

fn units(scope: &v8::PinScope, text: v8::Local<v8::String>) -> Vec<u16> {
    let mut result = vec![0; text.length()];
    text.write_v2(scope, 0, &mut result, v8::WriteFlags::empty());
    result
}
