#!/bin/sh
# Re-vendor the turbo-test DOM binding into turbo-surf-render.
#
# turbo-surf's JS-render tier reuses turbo-test's rtdom↔V8 browser binding
# (`browser_env.{rs,js}`). There is intentionally NO cross-repo crate dependency
# (turbo-test is a test runner, not a published lib), so the copy is kept in sync
# MANUALLY — by running this script whenever turbo-test's binding gains coverage.
#
# It is the entire sync story:
#   - browser_env.js          → copied byte-for-byte.
#   - browser_env_upstream.rs → copied with ONE mechanical transform: leading
#       inner-doc lines `//!` become `//`, because src/browser_env.rs `include!`s
#       this file and `include!` forbids a file beginning with inner attributes.
#       No code is changed.
#   - cssom_parser.rs → copied byte-for-byte from canonical upstream.
#   - html_documents.rs + browser_sanitizer_default.json → copied byte-for-byte.
#   - browser_env.rs → generated from browser-env-adapter.rs.in, revision embedded.
#
# Usage (from this crate's dir, with the turbo-test checkout beside turbo-surf):
#   sh scripts/vendor-browser-env.sh [path-to-turbo-test-repo]
set -eu

# Vendor from the turbo-test repo's LAST COMMIT (HEAD), never the dirty working
# tree — so a half-finished local edit in turbo-test can't leak into our vendored
# copy. Pass the turbo-test repo root (default: sibling of turbo-surf).
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo="${1:-$script_dir/../../../../../turbo-test}"
dest="$(CDPATH= cd -- "$(dirname -- "$0")/../src" && pwd)"

git -C "$repo" rev-parse --short HEAD >/dev/null 2>&1 || {
  echo "not a git repo (need turbo-test's HEAD): $repo" >&2
  exit 1
}
# Resolve once: another canonical commit cannot mix revisions during this copy.
source_rev="$(git -C "$repo" rev-parse --verify 'HEAD^{commit}')"
rev="$(git -C "$repo" rev-parse --short "$source_rev")"

git -C "$repo" show "${source_rev}:src/browser_env.js" > "$dest/browser_env.js"
git -C "$repo" show "${source_rev}:src/cssom_parser.rs" > "$dest/cssom_parser.rs"
git -C "$repo" show "${source_rev}:src/css_policy.rs" > "$dest/css_policy.rs"
for file in form_controls.js form_methods_cases.json form_methods_probe.js form_methods_chrome155.json form_entrylist_cases.json form_entrylist_probe.js form_entrylist_chrome155.json formdata_constructor_cases.json formdata_constructor_probe.js formdata_constructor_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in css_selector_lexemes_probe.js css_selector_lexemes_chrome155.json abstract_ranges_probe.js abstract_ranges_chrome155.json abstract_ranges_frames_probe.js abstract_ranges_frames_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in trusted_types_mapping.json trusted_types_sinks_probe.js trusted_types_cases.json trusted_types_conversion_probe.js trusted_types_chrome155.json trusted_types_matrix_chrome155.json trusted_types_matrix_probe.js trusted_types_sinks_cases.json trusted_types.js trusted_types_probe.js trusted_types_sinks_chrome155.json trusted_types.rs trusted_types_conversion_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in element_internals.rs element_internals.js checkedness_probe.js checkedness_chrome155.json element_internals_core_cases.json element_internals_core_probe.js element_internals_core_chrome155.json element_internals_associated_cases.json element_internals_associated_probe.js element_internals_associated_chrome155.json element_internals_state_cases.json element_internals_state_probe.js element_internals_state_chrome155.json element_internals_definition_cases.json element_internals_definition_probe.js element_internals_definition_chrome155.json element_internals_setlike_cases.json element_internals_setlike_probe.js element_internals_setlike_chrome155.json formdata_event_cases.json formdata_event_probe.js formdata_event_chrome155.json formdata_guards_cases.json formdata_guards_probe.js formdata_guards_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in form_connectivity_cases.json form_connectivity_probe.js form_connectivity_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in form_reflection_cases.json form_reflection_probe.js form_reflection_chrome155.json control_prototypes_probe.js control_prototypes_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in submitter_reflection_cases.json submitter_reflection_probe.js submitter_reflection_chrome155.json element_registry_probe.js element_registry_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in form_collections.js form_collections_cases.json form_collections_chrome155.json form_collections_lifecycle_chrome155.json form_collections_lifecycle_probe.js form_collections_probe.js form_collections_realm_chrome155.json form_collections_realm_probe.js; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in form_checked_state.rs radio_dirty_adoption_gc_chrome155.json radio_dirty_adoption_gc_probe.js radio_dirty_cases.json radio_dirty_chrome155.json radio_dirty_gc_chrome155.json radio_dirty_gc_probe.js radio_dirty_probe.js radio_dirty_transition_chrome155.json radio_dirty_transition_probe.js; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in control_labels.rs control_labels_probe.js control_labels_chrome155.json control_labels_realm_probe.js control_labels_realm_chrome155.json control_labels_gc_probe.js control_labels_gc_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in radio_group_moves_probe.js radio_group_moves_chrome155.json radio_owner_review_probe.js radio_owner_review_chrome155.json radio_dirty_adoption_gc_review_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in css_descriptor_cases.json css_descriptor_probe.js css_descriptor_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in computed_intrinsics_cases.json computed_intrinsics_probe.js computed_intrinsics_chrome155.json computed_intrinsics_font_probe.js computed_intrinsics_font_chrome155.json computed_intrinsics_font_realms_probe.js computed_intrinsics_font_realms_chrome155.json computed_button_ua_probe.js computed_button_ua_chrome155.json computed_border_priority_probe.js computed_border_priority_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
git -C "$repo" show "${source_rev}:src/event_handlers.rs" > "$dest/event_handlers.rs"
git -C "$repo" show "${source_rev}:src/event_brands.rs" > "$dest/event_brands.rs"
git -C "$repo" show "${source_rev}:src/event_listeners.rs" > "$dest/event_listeners.rs"
git -C "$repo" show "${source_rev}:src/event_listeners.js" > "$dest/event_listeners.js"
for file in range_contents.rs range_contents.js range_probe.js range_chrome155.json range_live.rs range_live_probe.js range_live_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in css_adoption_lifecycle_probe.js css_adoption_lifecycle_chrome155.json shadow_slots.rs shadow_slots.js shadow_slots_tests.rs shadow_slots_probe.js shadow_slots_chrome155.json frame_tokens_tests.rs frame_tokens_probe.js frame_tokens_chrome155.json iframe_credentialless_tests.rs iframe_credentialless_probe.js iframe_credentialless_chrome155.json html_meta_tests.rs html_meta_probe.js html_meta_chrome155.json css_live_tests.rs css_live_probe.js css_live_chrome155.json computed_color_probe.js computed_color_chrome155.json css_adoption_tests.rs css_adoption_probe.js css_adoption_ownership_probe.js css_adoption_chrome155.json css_adoption_ownership_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
git -C "$repo" show "${source_rev}:src/html_documents.rs" > "$dest/html_documents.rs"
git -C "$repo" show "${source_rev}:src/document_basics.rs" > "$dest/document_basics.rs"
git -C "$repo" show "${source_rev}:src/collection_records.rs" > "$dest/collection_records.rs"
git -C "$repo" show "${source_rev}:src/document_basics.js" > "$dest/document_basics.js"
for file in html_meta_tests.rs html_meta_probe.js html_meta_chrome155.json frame_tokens_tests.rs frame_tokens_probe.js frame_tokens_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in dom_implementation.rs dom_implementation.js dom_implementation_tests.rs dom_implementation_probe.js dom_implementation_chrome155.json xml_factory_elements_probe.js xml_factory_elements_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
git -C "$repo" show "${source_rev}:src/document_xpath.rs" > "$dest/document_xpath.rs"
git -C "$repo" show "${source_rev}:src/document_xpath.js" > "$dest/document_xpath.js"
mkdir -p "$dest/xpath_provider"
for file in xpath.js LICENSE README.md; do
  git -C "$repo" show "${source_rev}:src/xpath_provider/$file" > "$dest/xpath_provider/$file"
done
git -C "$repo" show "${source_rev}:src/element_tokens.js" > "$dest/element_tokens.js"
git -C "$repo" show "${source_rev}:src/element_relations.js" > "$dest/element_relations.js"
git -C "$repo" show "${source_rev}:src/element_relations.rs" > "$dest/element_relations.rs"
git -C "$repo" show "${source_rev}:src/html_text.rs" > "$dest/html_text.rs"
git -C "$repo" show "${source_rev}:src/html_text.js" > "$dest/html_text.js"
git -C "$repo" show "${source_rev}:src/html_popover.js" > "$dest/html_popover.js"
git -C "$repo" show "${source_rev}:src/html_popover.rs" > "$dest/html_popover.rs"
git -C "$repo" show "${source_rev}:src/dom_mutations.rs" > "$dest/dom_mutations.rs"
for file in dom_adoption_probe.js dom_adoption_chrome155.json dom_projection_tests.rs element_interface_probe.js element_interface_chrome155.json dom_variadic_errors_probe.js dom_variadic_errors_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in template_contents.rs template_contents.js template_ownership_probe.js template_ownership_chrome155.json template_lifecycle_probe.js template_lifecycle_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in custom_elements_retired_probe.js custom_elements_retired_chrome155.json custom_elements_callbacks_probe.js custom_elements_callbacks_chrome155.json dom_parser_observer_lifecycle_probe.js dom_parser_observer_lifecycle_chrome155.json custom_elements.rs custom_elements.js custom_elements_probe.js custom_elements_chrome155.json custom_elements_extended_probe.js custom_elements_extended_chrome155.json custom_elements_reactions_probe.js custom_elements_reactions_chrome155.json custom_elements_idl_probe.js custom_elements_idl_chrome155.json custom_elements_realms_probe.js custom_elements_realms_chrome155.json custom_elements_moves_probe.js custom_elements_moves_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in script_state.rs script_preparation_probe.js script_preparation_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in script_text.js script_text_probe.js script_text_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in utf16_contract_tests.rs utf16_probe.js utf16_chrome155.json utf16_bom_probe.js utf16_bom_chrome155.json utf16_handlers_probe.js utf16_handlers_chrome155.json utf16_lookups_probe.js utf16_lookups_chrome155.json utf16_metadata_probe.js utf16_metadata_chrome155.json utf16_names_probe.js utf16_names_chrome155.json utf16_parser_probe.js utf16_parser_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
for file in dom_ownership.rs realm_ownership.rs storage_ownership.rs dom_isolation.rs ownership_guard_tests.rs mutation_observers.rs mutation_observers.js mutation_observers_tests.rs mutation_observers_probe.js mutation_observers_chrome155.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
# turbo-surf#28 parity batches 1+2 — all vendored binding modules, probes and
# Chrome-156 controls (meter/progress/output, Element members, invokers, dialog,
# shadow HTML + Sanitizer, input/textarea + file inputs, selectors, Document members,
# select API + collections, pointer, animations, typed OM, reference/interest/focusgroup,
# interceptor modes, form pseudo-classes, key order, view transitions).
for file in form_outputs.js form_outputs_cases.json form_outputs_chrome156.json form_outputs_probe.js form_outputs_tests.rs element_members.js element_members.rs element_members_chrome156.json element_members_probe.js html_invokers.js html_invokers.rs html_invokers_chrome156.json html_invokers_dialog_chrome156.json html_invokers_dialog_probe.js html_invokers_probe.js shadow_html.js shadow_html.rs shadow_html_chrome156.json shadow_html_probe.js input_members.js input_members.rs input_members_cases.json input_members_chrome156.json input_members_probe.js css_selector_syntax.rs css_admission_tests.rs css_selector_admission_probe.js css_selector_admission_chrome156.json css_inherited_color_probe.js css_inherited_color_chrome156.json document_legacy.rs document_legacy.js document_legacy_tests.rs document_all_probe.js document_all_cases.json document_all_chrome156.json document_legacy_probe.js document_legacy_cases.json document_legacy_chrome156.json select_element.rs select_element.js select_element_tests.rs select_element_probe.js select_element_cases.json select_element_chrome156.json html_dialog.rs html_dialog.js html_dialog_tests.rs html_dialog_probe.js html_dialog_chrome156.json sanitizer.rs sanitizer.js sanitizer_safe_baseline.json sanitizer_tests.rs sanitizer_probe.js sanitizer_chrome156.json declarative_shadow_probe.js declarative_shadow_page.html declarative_shadow_chrome156.json pointer_events.rs pointer_events.js pointer_events_tests.rs pointer_events_probe.js pointer_events_chrome156.json animations.rs animations.js animations_tests.rs animations_probe.js animations_cases.json animations_chrome156.json animations_frames_probe.js animations_frames_cases.json animations_frames_chrome156.json file_inputs.rs file_inputs.js file_inputs_tests.rs file_inputs_probe.js file_inputs_cases.json file_inputs_chrome156.json input_sanitization_probe.js input_sanitization_cases.json input_sanitization_chrome156.json reference_target.rs reference_target.js reference_target_tests.rs reference_target_probe.js reference_target_chrome156.json probe_harness_tests.rs interest_invokers.rs interest_invokers.js interest_invokers_tests.rs interest_invokers_probe.js interest_invokers_chrome156.json focus_group.rs focus_group.js focus_group_tests.rs focus_group_probe.js focus_group_chrome156.json typed_om.rs typed_om.js typed_om_tests.rs typed_om_probe.js typed_om_cases.json typed_om_chrome156.json typed_om_properties.json typed_om_registry_probe.js typed_om_registry_cases.json typed_om_registry_chrome156.json hyperlink_utils.rs hyperlink_utils.js interface_members.js interface_members_probe.js interface_members_cases.json interface_members_chrome156.json control_interfaces_tests.rs control_interfaces_probe.js control_interfaces_cases.json control_interfaces_chrome156.json interceptor_mode.rs interceptor_mode_probe.js interceptor_mode_cases.json interceptor_mode_chrome156.json form_pseudo.rs form_pseudo.js form_pseudo_tests.rs form_pseudo_probe.js form_pseudo_cases.json form_pseudo_chrome156.json key_order.rs key_order.js key_order_tests.rs key_order_probe.js key_order_chrome156.json view_transitions.rs view_transitions.js view_transitions_tests.rs view_transitions_probe.js view_transitions_hidden_probe.js view_transitions_chrome156.json user_activation_gates.rs user_activation_gates.js user_activation_gates_probe.js user_activation_gates_chrome156.json; do
# turbo-surf#28 parity batches 1+2 (meter/progress/output, Element members, invokers,
# declarative shadow HTML, input/textarea, strict selectors, legacy Document members,
# select API, HTMLDialogElement, Sanitizer, pointer capture/lock, Web Animations,
# file inputs/DataTransfer, reference targets/interest/focusgroup, CSS Typed OM, ...).
for file in form_outputs.js form_outputs_cases.json form_outputs_chrome156.json form_outputs_probe.js form_outputs_tests.rs element_members.js element_members.rs element_members_chrome156.json element_members_probe.js html_invokers.js html_invokers.rs html_invokers_chrome156.json html_invokers_dialog_chrome156.json html_invokers_dialog_probe.js html_invokers_probe.js shadow_html.js shadow_html.rs shadow_html_chrome156.json shadow_html_probe.js input_members.js input_members.rs input_members_cases.json input_members_chrome156.json input_members_probe.js css_selector_syntax.rs css_admission_tests.rs css_selector_admission_probe.js css_selector_admission_chrome156.json css_inherited_color_probe.js css_inherited_color_chrome156.json document_legacy.rs document_legacy.js document_legacy_tests.rs document_all_probe.js document_all_cases.json document_all_chrome156.json document_legacy_probe.js document_legacy_cases.json document_legacy_chrome156.json select_element.rs select_element.js select_element_tests.rs select_element_probe.js select_element_cases.json select_element_chrome156.json html_dialog.rs html_dialog.js html_dialog_tests.rs html_dialog_probe.js html_dialog_chrome156.json sanitizer.rs sanitizer.js sanitizer_safe_baseline.json sanitizer_tests.rs sanitizer_probe.js sanitizer_chrome156.json declarative_shadow_probe.js declarative_shadow_page.html declarative_shadow_chrome156.json pointer_events.rs pointer_events.js pointer_events_tests.rs pointer_events_probe.js pointer_events_chrome156.json animations.rs animations.js animations_tests.rs animations_probe.js animations_cases.json animations_chrome156.json animations_frames_probe.js animations_frames_cases.json animations_frames_chrome156.json file_inputs.rs file_inputs.js file_inputs_tests.rs file_inputs_probe.js file_inputs_cases.json file_inputs_chrome156.json input_sanitization_probe.js input_sanitization_cases.json input_sanitization_chrome156.json reference_target.rs reference_target.js reference_target_tests.rs reference_target_probe.js reference_target_chrome156.json probe_harness_tests.rs interest_invokers.rs interest_invokers.js interest_invokers_tests.rs interest_invokers_probe.js interest_invokers_chrome156.json focus_group.rs focus_group.js focus_group_tests.rs focus_group_probe.js focus_group_chrome156.json typed_om.rs typed_om.js typed_om_tests.rs typed_om_probe.js typed_om_cases.json typed_om_chrome156.json typed_om_properties.json typed_om_registry_probe.js typed_om_registry_cases.json typed_om_registry_chrome156.json user_activation_gates.rs user_activation_gates.js user_activation_gates_probe.js user_activation_gates_chrome156.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
# turbo-surf#28 batch 2 (interfaces): HTMLOptionsCollection/control prototypes, hyperlink utils
# and small interface members.
for file in hyperlink_utils.rs hyperlink_utils.js interface_members.js interface_members_probe.js interface_members_cases.json interface_members_chrome156.json control_interfaces_tests.rs control_interfaces_probe.js control_interfaces_cases.json control_interfaces_chrome156.json; do
  git -C "$repo" show "${source_rev}:src/$file" > "$dest/$file"
done
git -C "$repo" show "${source_rev}:src/browser_sanitizer_default.json" > "$dest/browser_sanitizer_default.json"
git -C "$repo" show "${source_rev}:src/browser_env.rs" | sed 's#^//!#//#' > "$dest/browser_env_upstream.rs"

sed "s/@TURBO_TEST_REV@/$rev/g" "$(dirname -- "$0")/browser-env-adapter.rs.in" > "$dest/browser_env.rs"

echo "vendored browser_env.{js,rs} ← $repo @ $rev (committed $source_rev)."
echo "Adapter generated from template. Verify: cargo test -p turbo-surf-render"
