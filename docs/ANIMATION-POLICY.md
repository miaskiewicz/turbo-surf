# Animation presentation policy

The session option `animations` accepts `enabled` (the default) or `disabled`.
It controls the presentation of canonical CSS animations, CSS transitions and
Web Animations effects. It is independent of reduced-motion media and the
screenshot API's capture options.

With `disabled`, computed styles and native geometry use underlying declarations
instead of composing animation effects. For example, a spinner whose underlying
`transform` is `none` stays unrotated, and a transitioning opacity immediately
presents its newly declared value. This applies to effects declared in the initial
HTML, effects created later, paused effects, infinite effects and filled effects.
It does not require painting intermediate keyframes.

Animation objects and scheduling remain genuine. Finite effects resolve their
`ready` and `finished` promises and dispatch events on their normal canonical
schedule. Infinite effects remain unfinished; disabling presentation does not
forge their completion. No effect is cancelled or fast-forwarded by this option.
Web Animations interfaces remain available, including `Element.animate()`,
`getAnimations()` and `document.timeline`.

Ordinary JavaScript keeps running. Timers, network callbacks and
`requestAnimationFrame` callbacks retain their behavior, including application
code that writes styles itself. The policy suppresses canonical effect
composition; it does not intercept arbitrary JavaScript style writes.

The choice is frozen before scripts execute in a native session. Child Windows
inherit that session's choice. Other sessions and contexts are independent.
There is no live Page or BrowserContext policy setter: create a new session or
context with `enabled` to restore normal effect presentation. Updates to unrelated
preferences do not change an existing session's choice.

## Playwright facade

```js
const context = await browser.newContext({ animations: 'disabled' });
const page = await context.newPage();
```

The same option works with `browser.newPage({ animations: 'disabled' })` and the
runner's project `use` configuration. Unknown values fail before a context's
native resources are allocated.

## Rust and NAPI

Rust callers can select a policy on `PageSessionOptions`:

```rust
use turbo_surf_render::{AnimationPolicy, PageSessionOptions};

let options = PageSessionOptions {
    animation_policy: Some(AnimationPolicy::Disabled),
    ..Default::default()
};
```

An explicit session option overrides its browsing profile. Otherwise the profile
supplies the choice. `BrowserStorage::set_animation_policy` configures future
sessions without replacing other preferences. `configure_emulation` also accepts
an `animations` member in its existing JSON configuration.

NAPI callers use that same member through `requestStateConfigure`; live sessions
freeze their profile choice when they open. The Playwright facade forwards it to
both the context and page request states.

## MCP

Select presentation together with the render mode:

```json
{"name":"set_mode","arguments":{"mode":"fast","animations":"disabled"}}
```

When `animations` is supplied, the result acknowledges the selected `mode` and
`animations`. Mode-only calls retain their original result shape and the session's
configured animation choice. MCP applies the choice to future render
runtimes, including subsequent `render` calls. Invalid policies leave the
configuration unchanged.

## Geometry admission

A fresh producer observation must identify the actual element and document and
prove presentation suppression before native geometry admits underlying styles.
This is a distinct proof from naturally completed, non-filling animations in the
normal enabled mode. Enabled-mode name agreement and all existing font, transform,
resource and ownership checks remain in place. Missing or failed policy proof
retains the original animation guard, and lifecycle-waived layouts are not cached.
