# Native host media environment

CSS layout and accessibility/pointer `matchMedia` queries read the same immutable host environment captured before page scripts. The layout cache key includes that environment. Public Window getters do not supply its viewport or preference values.

On macOS, contrast, reduced motion and reduced transparency come from the real NSWorkspace accessibility APIs. Color scheme comes from the system appearance preference. The native macOS view uses system colors with no forced page-color override, so forced-colors is none. Fine pointer and hover are capabilities of the installed native mouse input driver. Unavailable platform preferences remain explicit errors.

Independent Chrome155.0.8059.80 controls cover31 queries and actual CSS rectangles. The native runtime matched that fresh-profile result exactly; no media emulation, browser cookie reuse or extension was involved. Run the reusable `authored-host-media-chrome.mjs` collector to refresh the source-pinned fixture. `TURBO_SURF_VALIDATE_SAVED_HOST_MEDIA=1 cargo test -p turbo-surf-render --test host_media` additionally checks the saved host-specific result; regular tests check host-independent invariants and CSS/query agreement, allowing machines with different genuine accessibility preferences.

Remaining coverage: OS preference change notifications and MediaQueryList change events, custom browser-profile color overrides, non-macOS OS preference backends and unrelated missing media features. Other legacy matchMedia queries still use the existing evaluator. Active forced-color palette rendering is not advertised. These limitations remain tracked in issue28.
