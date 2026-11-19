use serde_json::{json, Value};
use turbo_surf_render::run_with_dom;

const HTML: &str = include_str!("fixtures/animation_offset_queries.html");

fn report(script: &str) -> Value {
    serde_json::from_str(&run_with_dom(HTML, script).unwrap()).unwrap()
}

#[test]
fn popper_offset_reads_ignore_an_unrelated_transform_animation() {
    let actual = report(
        r#"(() => {
          const probe = document.getElementById('probe');
          const read = () => ({left:probe.offsetLeft, top:probe.offsetTop, parent:probe.offsetParent.id});
          const baseline = read();
          document.getElementById('dot').style.animation = 'pulse 1s infinite';
          return JSON.stringify({baseline, animated:read()});
        })()"#,
    );
    let expected = json!({"left":11,"top":7,"parent":"container"});
    assert_eq!(actual["baseline"], expected);
    assert_eq!(actual["animated"], expected);
}

#[test]
fn mui_keyboard_highlight_can_read_the_options_untransformed_offsets() {
    let actual = report(
        r#"(() => {
          document.getElementById('dot').style.animation = 'pulse 1s infinite';
          const listboxNode = document.getElementById('listbox');
          const element = document.getElementById('selected');
          let desired = null;
          // MUI useAutocomplete's keyboard highlight computes a scroll target
          // from these reads before it performs any actual movement.
          if (listboxNode.scrollHeight > listboxNode.clientHeight) {
            const scrollBottom = listboxNode.clientHeight + listboxNode.scrollTop;
            const elementBottom = element.offsetTop + element.offsetHeight;
            if (elementBottom > scrollBottom) desired = elementBottom - listboxNode.clientHeight;
          }
          return JSON.stringify({desired, top:element.offsetTop, parent:element.offsetParent.id, scrollTop:listboxNode.scrollTop});
        })()"#,
    );
    assert_eq!(
        actual,
        json!({"desired":10,"top":20,"parent":"listbox","scrollTop":0})
    );
}

#[test]
fn offset_reads_keep_animated_targets_ancestors_and_mixed_effects_strict() {
    let actual = report(
        r#"(() => {
          const probe = document.getElementById('probe');
          const container = document.getElementById('container');
          const dot = document.getElementById('dot');
          const reads = () => ['offsetLeft','offsetTop','offsetParent'].map(name => {
            try { return {value:probe[name] === container ? 'container' : probe[name]}; }
            catch(error) { return {error:error.message}; }
          });
          probe.style.animation = 'pulse 1s infinite';
          const target = reads();
          probe.style.animation = '';
          container.style.animation = 'pulse 1s infinite';
          const ancestor = reads();
          container.style.animation = '';
          dot.style.animation = 'mixed 1s infinite';
          return JSON.stringify({target,ancestor,mixed:reads()});
        })()"#,
    );
    for phase in ["target", "ancestor", "mixed"] {
        for read in actual[phase].as_array().unwrap() {
            assert!(
                read["error"]
                    .as_str()
                    .unwrap()
                    .contains("animation resource"),
                "{phase}: {actual}"
            );
        }
    }
}
