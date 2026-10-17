//! Native activation brands and same-origin sticky state; no guest timestamps.
use super::*;

#[derive(Clone)]
pub struct NavigationActivationSnapshot {
    source: String,
    sticky: bool,
}

struct InheritedSticky(bool);

pub(super) fn has_been_active(context: v8::Local<v8::Context>) -> bool {
    context.get_slot::<HostUserActivation>().is_some()
        || context
            .get_slot::<InheritedSticky>()
            .is_some_and(|state| state.0)
}

impl NavigationActivationSnapshot {
    pub(super) fn capture(source: String, context: v8::Local<v8::Context>) -> Self {
        Self {
            source,
            sticky: has_been_active(context),
        }
    }

    fn carries_to(&self, destination: &str) -> bool {
        let (Ok(source), Ok(destination)) =
            (url::Url::parse(&self.source), url::Url::parse(destination))
        else {
            return false;
        };
        self.sticky
            && matches!(source.origin(), url::Origin::Tuple(..))
            && source.origin() == destination.origin()
    }
}

pub(super) fn seed(
    rt: &mut JsRuntime,
    destination: &str,
    snapshot: Option<NavigationActivationSnapshot>,
) {
    let sticky = snapshot.is_some_and(|snapshot| snapshot.carries_to(destination));
    deno_core::scope!(scope, rt);
    scope
        .get_current_context()
        .set_slot(Rc::new(InheritedSticky(sticky)));
}

fn key<'s>(scope: &mut v8::PinScope<'s, '_>, name: &str) -> Option<v8::Local<'s, v8::Private>> {
    let name = v8::String::new(scope, name)?;
    Some(v8::Private::for_api(scope, Some(name)))
}

#[op2(fast)]
pub(super) fn op_user_activation_install(
    scope: &mut v8::PinScope,
    navigator: v8::Local<v8::Object>,
    activation: v8::Local<v8::Object>,
) -> bool {
    let Some(brand) = key(scope, "turbo-surf.user-activation.brand") else {
        return false;
    };
    let yes = v8::Boolean::new(scope, true);
    if activation.set_private(scope, brand, yes.into()) != Some(true) {
        return false;
    }
    let Some(owner) = key(scope, "turbo-surf.user-activation.navigator") else {
        return false;
    };
    navigator.set_private(scope, owner, activation.into()) == Some(true)
}

#[op2]
pub(super) fn op_user_activation_navigator<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Value>,
) -> Result<v8::Local<'s, v8::Object>, JsErrorBox> {
    let receiver = v8::Local::<v8::Object>::try_from(receiver)
        .map_err(|_| JsErrorBox::type_error("Illegal invocation"))?;
    let owner = key(scope, "turbo-surf.user-activation.navigator")
        .ok_or_else(|| JsErrorBox::type_error("Activation state unavailable"))?;
    receiver
        .get_private(scope, owner)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))
}

#[op2(fast)]
pub(super) fn op_user_activation_read(
    scope: &mut v8::PinScope,
    receiver: v8::Local<v8::Value>,
    sticky: bool,
) -> Result<bool, JsErrorBox> {
    let receiver = v8::Local::<v8::Object>::try_from(receiver)
        .map_err(|_| JsErrorBox::type_error("Illegal invocation"))?;
    let brand = key(scope, "turbo-surf.user-activation.brand")
        .ok_or_else(|| JsErrorBox::type_error("Activation state unavailable"))?;
    if !receiver
        .get_private(scope, brand)
        .is_some_and(|value| value.is_true())
    {
        return Err(JsErrorBox::type_error("Illegal invocation"));
    }
    let context = receiver
        .get_creation_context(scope)
        .ok_or_else(|| JsErrorBox::type_error("Illegal invocation"))?;
    Ok(if sticky {
        has_been_active(context)
    } else {
        host_user_activation(context)
    })
}
