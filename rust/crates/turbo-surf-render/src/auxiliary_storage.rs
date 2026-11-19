use super::*;

struct ContextStorage(BrowserStorage);

fn top_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    mut context: v8::Local<'s, v8::Context>,
) -> Result<v8::Local<'s, v8::Context>, JsErrorBox> {
    for _ in 0..64 {
        let Some(parent) = context.get_slot::<FrameParent>() else {
            return Ok(context);
        };
        context = parent
            .0
            .to_local(scope)
            .ok_or_else(|| JsErrorBox::generic("Storage parent context has retired"))?;
    }
    Err(JsErrorBox::generic(
        "Storage frame ancestry exceeds native limit",
    ))
}

pub(super) fn backing<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &OpState,
    context: v8::Local<'s, v8::Context>,
) -> Result<BrowserStorage, JsErrorBox> {
    let context = top_context(scope, context)?;
    if let Some(storage) = context.get_slot::<ContextStorage>() {
        return Ok(storage.0.clone());
    }
    state
        .try_borrow::<BrowserStorage>()
        .cloned()
        .ok_or_else(|| JsErrorBox::generic("Storage backing unavailable"))
}

pub(super) fn site_source<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    state: &OpState,
    context: v8::Local<'s, v8::Context>,
) -> Result<String, JsErrorBox> {
    let context = top_context(scope, context)?;
    Ok(context
        .get_slot::<FrameDocumentUrl>()
        .map(|url| url.0.clone())
        .unwrap_or_else(|| state.borrow::<Base>().0.clone()))
}

pub(super) fn install_new<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    source: v8::Local<'s, v8::Context>,
    copy_session: bool,
) -> Result<(), JsErrorBox> {
    let state = JsRuntime::op_state_from(scope);
    let source = backing(scope, &state.borrow(), source)?;
    let storage = fork_tab(&source, copy_session).map_err(JsErrorBox::generic)?;
    context.set_slot(Rc::new(ContextStorage(storage)));
    Ok(())
}

pub(super) fn carry_navigation<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    previous: v8::Local<'s, v8::Context>,
    current: v8::Local<'s, v8::Context>,
) -> Result<(), JsErrorBox> {
    let state = JsRuntime::op_state_from(scope);
    let storage = backing(scope, &state.borrow(), previous)?;
    current.set_slot(Rc::new(ContextStorage(storage)));
    Ok(())
}

pub(super) fn retire(context: v8::Local<v8::Context>) {
    context.remove_slot::<ContextStorage>();
}

fn fork_tab(source: &BrowserStorage, copy_session: bool) -> Result<BrowserStorage, String> {
    let mut destination = source.new_tab()?;
    // Popup Pages share BrowserContext preferences, not an emulation snapshot.
    destination.4 = source.4.clone();
    if copy_session {
        copy_session_areas(source, &destination)?;
    }
    Ok(destination)
}

fn copy_session_areas(source: &BrowserStorage, destination: &BrowserStorage) -> Result<(), String> {
    let mut areas = source.0.lock().map_err(|_| "Storage backing unavailable")?;
    let snapshot = areas
        .by_origin
        .iter()
        .filter(|(key, _)| key.session && key.tab == source.2.id)
        .map(|(key, id)| {
            (
                key.clone(),
                areas.values.get(id).cloned().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    let count = u32::try_from(snapshot.len()).map_err(|_| "Too many storage areas")?;
    areas
        .next
        .checked_add(count)
        .ok_or("Too many storage areas")?;
    // HTML's legacy session-shed clone is a snapshot. Values and future writes
    // belong to the new traversable; local storage still uses shared tab zero.
    for (mut key, values) in snapshot {
        areas.next += 1;
        let id = areas.next;
        key.tab = destination.2.id;
        areas.by_origin.insert(key, id);
        areas.values.insert(id, values);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(storage: &BrowserStorage, origin: &str, session: bool) -> u32 {
        let mut areas = storage.0.lock().unwrap();
        areas.next += 1;
        let id = areas.next;
        areas.by_origin.insert(
            StorageKey {
                partition: 0,
                site: origin.into(),
                origin: origin.into(),
                cross_site_ancestor: false,
                session,
                tab: if session { storage.2.id } else { 0 },
            },
            id,
        );
        areas.values.insert(id, vec![(vec![1], vec![2])]);
        id
    }

    #[test]
    fn copies_all_session_origins_without_aliasing_values() {
        let source = BrowserStorage::default();
        let original = seed(&source, "https://one.test", true);
        seed(&source, "https://two.test", true);
        let local = seed(&source, "https://one.test", false);
        let child = fork_tab(&source, true).unwrap();
        let mut areas = source.0.lock().unwrap();
        let copied = areas
            .by_origin
            .iter()
            .filter(|(key, _)| key.session && key.tab == child.2.id)
            .map(|(_, id)| *id)
            .collect::<Vec<_>>();
        assert_eq!(copied.len(), 2);
        areas.values.get_mut(&copied[0]).unwrap()[0].1 = vec![3];
        assert_eq!(areas.values[&original][0].1, vec![2]);
        assert_eq!(areas.values[&local][0].1, vec![2]);
    }

    #[test]
    fn noopener_is_empty_but_shares_context_preferences_and_profile() {
        let source = BrowserStorage::default();
        seed(&source, "https://one.test", true);
        let child = fork_tab(&source, false).unwrap();
        assert!(std::sync::Arc::ptr_eq(&source.0, &child.0));
        assert!(std::sync::Arc::ptr_eq(&source.4, &child.4));
        assert_ne!(source.2.id, child.2.id);
        assert!(source
            .0
            .lock()
            .unwrap()
            .by_origin
            .keys()
            .all(|key| key.tab != child.2.id));
    }

    #[test]
    fn area_exhaustion_leaves_the_source_snapshot_intact() {
        let source = BrowserStorage::default();
        let original = seed(&source, "https://one.test", true);
        source.0.lock().unwrap().next = u32::MAX;
        assert!(fork_tab(&source, true).is_err());
        let areas = source.0.lock().unwrap();
        assert_eq!(areas.by_origin.len(), 1);
        assert_eq!(areas.values.len(), 1);
        assert_eq!(areas.values[&original][0].1, vec![2]);
    }

    #[test]
    fn closing_a_child_releases_only_its_session_snapshot() {
        let source = BrowserStorage::default();
        let original = seed(&source, "https://one.test", true);
        let local = seed(&source, "https://one.test", false);
        let child = fork_tab(&source, true).unwrap();
        assert_eq!(source.0.lock().unwrap().values.len(), 3);
        drop(child);
        let areas = source.0.lock().unwrap();
        assert_eq!(areas.values.len(), 2);
        assert!(areas.values.contains_key(&original));
        assert!(areas.values.contains_key(&local));
    }
}
