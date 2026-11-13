// Actor preference handles survive the opener's Page/request-state alias. This
// synchronous host write must precede any command awaiting that actor's V8 work.
export function configureActorGeolocation(native, actors, geolocation, permissions) {
  if (!actors?.size) return;
  const options = JSON.stringify({ geolocation, permissions });
  for (const actor of actors.values())
    if (actor.active) native.liveActorConfigureEmulation(actor.actor, options);
}
