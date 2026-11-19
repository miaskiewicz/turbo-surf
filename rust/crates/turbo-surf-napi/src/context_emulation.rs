//! Context settings reach the surviving actor without queueing behind page JS.
use napi::bindgen_prelude::*;
use napi_derive::napi;

#[napi]
pub fn live_actor_configure_emulation(actor: u32, options_json: String) -> Result<()> {
    crate::session::actor_context_emulation(actor)?
        .configure_geolocation(&options_json)
        .map_err(Error::from_reason)
}
