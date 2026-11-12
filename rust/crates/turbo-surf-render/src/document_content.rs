use super::*;

#[op2(reentrant)]
pub(super) fn op_replace_document_content<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    document: v8::Local<'s, v8::Object>,
    html: v8::Local<'s, v8::String>,
) -> Result<v8::Local<'s, v8::Array>, JsErrorBox> {
    let units = crate::browser_env::string_code_units(scope, html);
    crate::live_layout_backend::clear_document_images(scope, document);
    let scripts = crate::browser_env::replace_document_content(scope, document, &units)
        .map_err(JsErrorBox::generic)?;
    let values = scripts
        .into_iter()
        .map(Into::into)
        .collect::<Vec<v8::Local<v8::Value>>>();
    Ok(v8::Array::new_with_elements(scope, &values))
}

impl PageSession {
    /// Replace the active document contents while keeping its Window and native document.
    pub async fn set_content(
        &mut self,
        html: &str,
        wait_until: &str,
        timeout_ms: Option<u64>,
    ) -> Result<String, String> {
        if self.closed {
            return Err("Page session is closed".into());
        }
        if !matches!(wait_until, "load" | "domcontentloaded" | "networkidle") {
            return Err(format!("Unsupported setContent waitUntil: {wait_until}"));
        }
        let budget = timeout_ms
            .filter(|value| *value > 0)
            .unwrap_or(self.budget_ms);
        let watchdog = Watchdog::start(self.rt.v8_isolate().thread_safe_handle(), budget);
        let key = self.rt.activation_key();
        let result = key
            .during(tokio::time::timeout(
                std::time::Duration::from_millis(budget),
                self.set_content_active(html, wait_until),
            ))
            .await;
        let exceeded = watchdog.exceeded.load(std::sync::atomic::Ordering::Relaxed);
        drop(watchdog);
        self.rt.v8_isolate().cancel_terminate_execution();
        if exceeded || result.is_err() {
            return Err(format!("setContent timeout after {budget}ms"));
        }
        result.unwrap()?;
        self.follow_form_navigation().await?;
        Ok(self.serialize())
    }

    async fn set_content_active(&mut self, html: &str, wait_until: &str) -> Result<(), String> {
        let content = {
            deno_core::scope!(scope, self.rt);
            let value: v8::Local<v8::Value> = v8::String::new(scope, html)
                .ok_or("Document content allocation failed")?
                .into();
            v8::Global::new(scope, value)
        };
        namespace_call(&mut self.rt, "resetTimerBudget", &[])?;
        run_lifecycle(&mut self.rt, "documentLoading")?;
        namespace_call(
            &mut self.rt,
            "replaceDocumentContent",
            &[HostArg::Value(content)],
        )?;
        namespace_call(&mut self.rt, "startInitialHydrate", &[])?;
        drain_event_loop_until(&mut self.rt, Some("initialHydrateDone")).await?;
        let base = self.rt.op_state().borrow().borrow::<Base>().0.clone();
        drain_module_scripts(&mut self.rt, &base).await?;
        run_lifecycle(&mut self.rt, "dispatchDocumentInteractive")?;
        if wait_until == "domcontentloaded" {
            namespace_call(&mut self.rt, "scheduleContentLoad", &[])?;
            return Ok(());
        }
        drain_event_loop_until(&mut self.rt, Some("documentLoadReady")).await?;
        run_lifecycle(&mut self.rt, "dispatchDocumentComplete")?;
        if wait_until == "networkidle" {
            drain_to_quiescence(&mut self.rt).await?;
        }
        Ok(())
    }
}
