# Geometry-neutral at-rule admission

Modern CSS rules share `CSSRule.type === 0`; the number alone never establishes that a rule is irrelevant. Native layout parses each rule with Lightning CSS and admits only the following bounded cases:

- A named `@container` rule is inactive when the active CSS and native inline declarations contain no container/name/type declaration. Any potentially eligible or unresolved container remains an explicit error. This is an absence proof, not a container-query implementation.
- A valid `<color>` `@property` rule may be omitted from the geometry cascade only when every transitive variable use terminates in background or color properties. The graph decodes CSS escapes, nested functions and fallbacks, scans important and inline declarations, and rejects width, padding, border shorthands, transforms and other terminals. This establishes geometry neutrality; it does not implement registration semantics.
- A navigation-only `@view-transition` rule does not change static document geometry. Active view-transition lifecycle, generated transition boxes and compositor behavior remain unsupported.
- Font/keyframe definitions retain their existing typed resource checks; used unsupported resources still fail. Unknown rule kinds, including layers, remain explicit errors.

The independent Chrome 155 control has 14 cases, including real eligible containers and direct/transitive/escaped geometry dependencies. Native tests compare admitted rectangles and rule types, require errors for unsupported counterexamples, and verify that subsequent inline/sheet mutations invalidate earlier absence proofs.

`TURBO_SURF_CAPTURED_CSS_REPLAY=/absolute/captured.css TURBO_SURF_CAPTURED_HTML_REPLAY=/absolute/captured.html cargo test -p turbo-surf-render --lib captured_css_dependency_graph_can_be_replayed_offline` replays the dependency proof against saved sources. It parses native trees and CSS without executing page scripts or accessing the network.

Canonical turbo-test 0ec37f5 retains valid property/view-transition descriptors in both CSSOM and the private native-layout snapshot. Descriptor retention is separate from full CSSPropertyRule interfaces, CSSPropertiesAndValues registration, CSS.registerProperty, inheritance/initial-value resolution and getComputedStyle behavior. Those features, container evaluation and transition lifecycle remain tracked in [issue 28](https://github.com/miaskiewicz/turbo-surf/issues/28).
