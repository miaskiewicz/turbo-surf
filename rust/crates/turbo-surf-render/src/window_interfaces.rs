//! Native halves of window_interfaces.js: the Location interface object with an
//! immutable-prototype instance (stock engine), and the native-proxy Location seed.
use super::*;

fn illegal_constructor(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let message = v8::String::new(scope, "Illegal constructor").unwrap();
    let error = v8::Exception::type_error(scope, message);
    scope.throw_exception(error);
}

/// `[Location, location]`: the interface object (read-only prototype) and this realm's
/// Location instance, whose [[SetPrototypeOf]] refuses any other prototype as in Chrome.
/// window_interfaces.js defines the [LegacyUnforgeable] own members on the instance.
#[op2]
pub(super) fn op_location_object<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Array> {
    let constructor = v8::FunctionTemplate::new(scope, illegal_constructor);
    let name = v8::String::new(scope, "Location").unwrap();
    constructor.set_class_name(name);
    constructor.read_only_prototype();
    let template = constructor.instance_template(scope);
    template.set_immutable_proto();
    let function = constructor.get_function(scope).unwrap();
    let instance = template.new_instance(scope).unwrap();
    v8::Array::new_with_elements(scope, &[function.into(), instance.into()])
}

/// Same-document URL updates (pushState, fragment navigation) move the native-proxy
/// Location's URL without a navigation. The stock engine keeps that URL in JS.
#[op2(fast)]
pub(super) fn op_location_seed(scope: &mut v8::PinScope, #[string] url: String) -> bool {
    #[cfg(turbo_surf_native_proxy)]
    {
        window_proxy::set_location_url(scope, &url);
        true
    }
    #[cfg(not(turbo_surf_native_proxy))]
    {
        let _ = (scope, url);
        false
    }
}

/// This realm's ancestor browsing contexts' origins, nearest parent first, for
/// Location.ancestorOrigins (the native-proxy build serves its own DOMStringList).
#[op2]
pub(super) fn op_location_ancestor_origins<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> v8::Local<'s, v8::Array> {
    let mut context = scope.get_current_context();
    let mut origins = Vec::new();
    while let Some(parent) = context.get_slot::<FrameParent>() {
        let Some(parent) = parent.0.to_local(scope) else {
            break;
        };
        context = parent;
        let origin = window_origin(scope, context);
        origins.push(v8::String::new(scope, &origin).unwrap().into());
    }
    v8::Array::new_with_elements(scope, &origins)
}
