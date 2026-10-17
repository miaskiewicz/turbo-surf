//! Optional host-only initial-hydration aggregates. Stages may overlap inclusively.
use deno_core::v8;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Stage {
    Capture,
    Composition,
    TreeSnapshot,
    Stylesheets,
    SheetExport,
    SelectorPreparation,
    RulesKey,
    Layout,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Aggregate {
    calls: u64,
    elapsed_microseconds: u64,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Records {
    #[serde(skip)]
    active: bool,
    stages: BTreeMap<Stage, Aggregate>,
    cache_hits: u64,
    cache_misses: u64,
}
#[derive(Clone)]
pub(crate) struct LayoutProfile(Rc<RefCell<Records>>);
impl LayoutProfile {
    pub(crate) fn install(isolate: &mut v8::Isolate) -> Self {
        let profile = Self(Rc::new(RefCell::new(Records {
            active: true,
            ..Records::default()
        })));
        isolate.set_slot(profile.clone());
        profile
    }
    pub(crate) fn current(scope: &v8::PinScope<'_, '_>) -> Option<Self> {
        scope
            .get_slot::<Self>()
            .filter(|profile| profile.0.borrow().active)
            .cloned()
    }
    pub(crate) fn cache(&self, hit: bool) {
        let mut records = self.0.borrow_mut();
        if hit {
            records.cache_hits = records.cache_hits.saturating_add(1);
        } else {
            records.cache_misses = records.cache_misses.saturating_add(1);
        }
    }
    pub(crate) fn stop(&self) -> serde_json::Value {
        let mut records = self.0.borrow_mut();
        records.active = false;
        serde_json::json!({
            "scope":"Initial hydration only; host stages overlap inclusively.",
            "timingUnit":"microseconds",
            "stages":records.stages,
            "cacheHits":records.cache_hits,
            "cacheMisses":records.cache_misses,
        })
    }
}
pub(crate) struct Timer {
    profile: LayoutProfile,
    stage: Stage,
    started: Instant,
}
impl Timer {
    pub(crate) fn new(scope: &v8::PinScope<'_, '_>, stage: Stage) -> Option<Self> {
        LayoutProfile::current(scope).map(|profile| Self {
            profile,
            stage,
            started: Instant::now(),
        })
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        let elapsed = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        let mut records = self.profile.0.borrow_mut();
        if records.active {
            let stage = records.stages.entry(self.stage).or_default();
            stage.calls = stage.calls.saturating_add(1);
            stage.elapsed_microseconds = stage.elapsed_microseconds.saturating_add(elapsed);
        }
    }
}
