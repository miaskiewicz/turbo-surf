//! Driver hydration yields between actual target-owned tasks, never inside one.
use super::*;
use turbo_dom_browser_env::host_internals::AuxiliaryContextId;

fn capability(rt: &mut JsRuntime, target: u64, name: &str) -> Result<String, String> {
    let value = auxiliary_pages::target_call(rt, AuxiliaryContextId(target), name, &[])?;
    read_string(rt, value)
}

struct Sample {
    pending: bool,
    fired: bool,
    busy: bool,
    signature: String,
}
impl Sample {
    fn idle(&self) -> bool {
        !self.pending && !self.fired && !self.busy
    }
    fn capture(
        rt: &mut JsRuntime,
        target: u64,
        turn: &async_owner::QuiescenceTurn,
    ) -> Result<Self, String> {
        let scripts = capability(rt, target, "interactionHydratePending")? == "1";
        let pending = capability(rt, target, "pendingWork")? == "1";
        let fetching = capability(rt, target, "pendingFetchCount")? != "0";
        let busy = turn.busy || fetching || scripts;
        let fired = turn.fired != 0;
        let signature = if busy || (!pending && !fired) {
            String::new()
        } else {
            capability(rt, target, "domSig")?
        };
        Ok(Self {
            pending,
            fired,
            busy,
            signature,
        })
    }
}

pub(super) async fn drain(rt: &mut JsRuntime, target: u64) -> Result<(), String> {
    let mut stable = 0;
    let mut signature = String::new();
    for _ in 0..500 {
        tokio::task::yield_now().await;
        // Keep actual script discovery/admission in its original target realm,
        // but leave every timer to the native, one-task cooperative scheduler.
        auxiliary_pages::target_call(
            rt,
            AuxiliaryContextId(target),
            "startInteractionHydrate",
            &[],
        )?;
        let turn = async_owner::quiescence_turn(rt, target, true).await?;
        if turn.host_reply {
            return Ok(());
        }
        let sample = Sample::capture(rt, target, &turn)?;
        if sample.idle() {
            return Ok(());
        }
        if sample.busy {
            stable = 0;
            continue;
        }
        if sample.signature == signature {
            stable += 1;
        } else {
            signature = sample.signature;
            stable = 0;
        }
        if stable >= 6 {
            return Ok(());
        }
    }
    Ok(())
}
