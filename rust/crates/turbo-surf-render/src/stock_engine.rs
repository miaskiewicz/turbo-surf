//! Default-profile secondary-realm module and code-generation services.
use super::v8;

pub(super) mod frame_modules {
    use super::super::*;

    pub(in crate::runtime) fn register_frame_modules<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        base: &str,
    ) -> Result<(), JsErrorBox> {
        let context = scope.get_current_context();
        if context.get_slot::<FrameParent>().is_some()
            || context.get_slot::<FrameRetired>().is_some()
            || context
                .get_slot::<FrameSandbox>()
                .is_some_and(|policy| policy.scripts_blocked)
        {
            return Ok(());
        }
        let loader = top_level_loader(scope, context, base)?;
        if !JsRuntime::register_module_context(scope, Rc::new(loader)) {
            return Err(JsErrorBox::generic(
                "Native Page module context unavailable",
            ));
        }
        context.set_slot(Rc::new(NativePageModuleContext));
        Ok(())
    }

    fn top_level_loader<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        context: v8::Local<'s, v8::Context>,
        base: &str,
    ) -> Result<NetModuleLoader, JsErrorBox> {
        let csp = context
            .get_slot::<CspHandle>()
            .ok_or_else(|| JsErrorBox::generic("Native Page module policy unavailable"))?;
        let source = context
            .get_slot::<FrameDocumentUrl>()
            .map_or_else(|| base.to_owned(), |source| source.0.clone());
        let credentials = ModuleCredentials::default();
        context.set_slot(Rc::new(credentials.clone()));
        let origins = Rc::new(ModuleOrigins::new(
            window_is_opaque(scope, context),
            frame_cookie_site_is_opaque(scope, context),
        ));
        context.set_slot(origins.clone());
        let state = JsRuntime::op_state_from(scope);
        let state = state.borrow();
        let storage = state.borrow::<BrowserStorage>();
        Ok(NetModuleLoader {
            network: state.borrow::<ModuleNetworkLog>().clone(),
            max_resource_bytes: state.borrow::<ResourceBudget>().0,
            base: base.to_owned(),
            source: source.clone(),
            top_level_source: source,
            origins,
            jar: csp.partition.clone().map_or_else(
                || state.borrow::<Jar>().clone(),
                |partition| partition.jar.clone(),
            ),
            ua: state.borrow::<Ua>().0.clone(),
            client_hints: state.borrow::<ClientHints>().clone(),
            http_cache: storage.http_cache(),
            storage_effects: storage.partition_response_effects(
                csp.partition.as_ref().map_or(0, |partition| partition.id),
            ),
            emulation: storage.4.clone(),
            inline_sources: state.borrow::<InlineModules>().clone(),
            credentials,
            csp: (*csp).clone(),
            blob_store: state.borrow::<blob_urls::Store>().clone(),
        })
    }

    #[op2(fast)]
    pub(in crate::runtime) fn op_frame_module(
        _window: v8::Local<v8::Object>,
        _index: u32,
    ) -> Result<(), JsErrorBox> {
        Err(JsErrorBox::generic(
            "Child-frame ES modules require the exact native SDK",
        ))
    }
}

pub(super) mod code_generation {
    use super::v8;

    pub(in crate::runtime) fn install_code_generation_authorization(
        _: &mut v8::PinScope,
    ) -> Result<(), String> {
        Ok(())
    }
}
