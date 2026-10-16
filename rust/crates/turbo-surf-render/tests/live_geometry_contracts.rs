use turbo_surf_render::run_with_dom;

#[test]
fn real_rectangle_interfaces_match_independent_chrome_shape_conversion_and_brand_control() {
    let probe = include_str!("../../../../harness/detection/geometry-interface-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/geometry-interface-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn rectangle_clone_preserves_exact_values_kind_aliases_and_private_ownership() {
    let actual=run_with_dom("<html><body></body></html>",r#"(()=>{
      const source=new DOMRect(-0,NaN,Infinity,-Infinity),readonly=new DOMRectReadOnly(2,3,4,5);
      Object.defineProperty(source,'x',{get(){throw Error('public shadow');}});
      const copy=structuredClone({source,alias:source,readonly});
      const valid=copy.source===copy.alias&&copy.source!==source&&Object.is(copy.source.x,-0)&&Number.isNaN(copy.source.y)&&copy.source.width===Infinity&&copy.source.height===-Infinity&&Object.getPrototypeOf(copy.source)===DOMRect.prototype&&Object.getPrototypeOf(copy.readonly)===DOMRectReadOnly.prototype;
      copy.source.x=22;
      const getter=Object.getOwnPropertyDescriptor(DOMRectReadOnly.prototype,'x').get;
      return String(valid&&getter.call(source)===0&&copy.source.x===22&&copy.readonly.x===2);
    })()"#).unwrap();
    assert_eq!(actual, "true");
}

#[test]
fn live_css_adoption_matches_independent_chrome_observable_array_control() {
    let probe = include_str!("../src/css_adoption_probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../src/css_adoption_chrome155.json")).unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn css_adoption_prototype_getters_keep_real_private_state_despite_document_shadowing() {
    let actual=run_with_dom("<html><body></body></html>",r#"(()=>{
      const sheet=new CSSStyleSheet();sheet.replaceSync('.a{width:20px}');
      const descriptor=Object.getOwnPropertyDescriptor(Document.prototype,'adoptedStyleSheets');
      document.adoptedStyleSheets=[sheet];
      let hooks=0;Object.defineProperty(document,'adoptedStyleSheets',{get(){hooks++;throw Error('public shadow');}});
      const actual=descriptor.get.call(document);
      let forged='';try{descriptor.get.call({});}catch(error){forged=error.name;}
      return String(actual.length===1&&actual[0]===sheet&&hooks===0&&forged==='TypeError');
    })()"#).unwrap();
    assert_eq!(actual, "true");
}

#[test]
fn real_live_element_geometry_tracks_cssom_adoption_mutations_and_private_ownership() {
    let probe = include_str!("../../../../harness/detection/live-element-geometry-probe.js")
        .trim()
        .trim_end_matches(';');
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/live-element-geometry-chrome155.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn adopted_sheet_shadow_ownership_brand_and_integrity_match_chrome() {
    let probe = include_str!("../src/css_adoption_ownership_probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../src/css_adoption_ownership_chrome155.json")).unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn genuine_native_document_implementation_matches_chrome_independent_factories_and_errors() {
    let probe = include_str!("../src/dom_implementation_probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../src/dom_implementation_chrome155.json")).unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn independent_document_factory_does_not_execute_poisoned_public_map_iterators() {
    let actual=run_with_dom("<!doctype html><html><head></head><body></body></html>",r#"(()=>{
      const descriptor=Object.getOwnPropertyDescriptor(Map.prototype,Symbol.iterator),define=Object.defineProperty;
      let hooks=0,created,failed;
      define(Map.prototype,Symbol.iterator,{value:function(){hooks++;throw Error('public map iterator');},configurable:true,writable:true});
      try{created=document.implementation.createHTMLDocument('private');}catch(error){failed=error.name;}
      finally{define(Map.prototype,Symbol.iterator,descriptor);}
      return JSON.stringify({hooks,created:!!created,failed:failed||null,title:created?.title});
    })()"#).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!({"hooks":0,"created":true,"failed":null,"title":"private"})
    );
}

#[test]
fn document_factories_bypass_poisoned_global_and_prototype_intrinsics() {
    let actual=run_with_dom("<!doctype html><html><head></head><body></body></html>",r#"(()=>{
      const define=Object.defineProperty,get=Object.getOwnPropertyDescriptor,apply=Reflect.apply;
      const saved=[[RegExp.prototype,'test'],[String.prototype,'split'],[String.prototype,'includes'],[Array.prototype,'some'],[globalThis,'Map'],[Object,'defineProperty']];
      const descriptors=[];for(let i=0;i<saved.length;i++)define(descriptors,i,{value:get(saved[i][0],saved[i][1]),configurable:true});
      const implementation=document.implementation;
      let hooks=0,html,xml,type,failed;
      const poison=function(){hooks++;throw Error('unexpected guest intrinsic');};
      for(let i=0;i<saved.length;i++)define(saved[i][0],saved[i][1],{value:poison,writable:true,configurable:true});
      try{html=implementation.createHTMLDocument('protected');type=implementation.createDocumentType('fixture','','');xml=implementation.createDocument('urn:fixture','p:Root',type);}
      catch(error){failed=error.name;}
      finally{for(let i=0;i<saved.length;i++)apply(define,Object,[saved[i][0],saved[i][1],descriptors[i]]);}
      return JSON.stringify({hooks,created:!!html&&!!xml,failed:failed||null,title:html?.title,owner:xml?.documentElement?.ownerDocument===xml,identity:xml?.doctype===type});
    })()"#).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        serde_json::json!({"hooks":0,"created":true,"failed":null,"title":"protected","owner":true,"identity":true})
    );
}

#[test]
fn parsed_html_nodes_keep_native_name_semantics_after_document_factory_installation() {
    let actual=run_with_dom("<!doctype html><html><head></head><body></body></html>",r#"(()=>{
      const doc=new DOMParser().parseFromString('<div id="native">x</div>','text/html');
      return String(doc.documentElement.nodeName==='HTML'&&doc.getElementById('native').tagName==='DIV'&&doc.body.ownerDocument===doc);
    })()"#).unwrap();
    assert_eq!(actual, "true");
}

#[test]
fn native_xml_element_factories_preserve_case_null_namespace_and_receiver_ownership() {
    let probe = include_str!("../../../../harness/detection/xml-factory-elements-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/xml-factory-elements-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn native_scroll_metrics_clamping_and_screen_projection_match_authored_chrome() {
    let probe = include_str!("../../../../harness/detection/scroll-layout-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/scroll-layout-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn native_hit_testing_matches_authored_chrome_transform_clip_visibility_and_pointer_controls() {
    let probe = include_str!("../../../../harness/detection/hit-layout-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/hit-layout-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn native_offset_positions_preserve_untransformed_padding_edge_and_actual_parent_ownership() {
    let probe = include_str!("../../../../harness/detection/offset-layout-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/offset-layout-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn native_geometry_rejects_forged_receivers_before_argument_conversion_and_bypasses_descriptor_poisoning(
) {
    let probe = include_str!("../../../../harness/detection/geometry-runtime-intrinsics-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/geometry-runtime-intrinsics-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<html><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn native_geometry_inert_xml_html_documents_and_disconnected_nodes_return_actual_empty_regions() {
    let probe = include_str!("../../../../harness/detection/inert-geometry-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/inert-geometry-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<html><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn native_geometry_edge_conversion_offset_rounding_and_device_pixel_scroll_match_chrome() {
    let probe = include_str!("../../../../harness/detection/geometry-edges-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/geometry-edges-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<html><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    let mut actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    let mut expected = expected["result"].clone();
    // The public control retains this exact real engine difference for further
    // LayoutUnit work; this contract covers the corrected integer conversions.
    for object in [&mut actual, &mut expected] {
        for row in object["offsets"].as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("rect");
        }
    }
    assert_eq!(actual, expected);
}

// This numeric Chrome control uses the same installed macOS Times face.
// Portable source-shaping controls use identical bundled font bytes in core.
#[cfg(target_os = "macos")]
#[test]
fn source_range_caret_interfaces_and_private_ownership_match_independent_chrome() {
    let probe = include_str!("../../../../harness/detection/live-text-geometry-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/live-text-geometry-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    let actual: serde_json::Value = serde_json::from_str(&actual).unwrap();
    assert_eq!(actual, expected["result"]);
}

#[test]
fn mixed_outer_svg_uses_native_namespace_and_real_source_geometry() {
    let actual = run_with_dom("<!doctype html><html><head><style>body{margin:0}svg{display:block}</style></head><body><svg id='icon' width='64.51' height='21.51'><rect id='shape' width='10' height='10'/></svg></body></html>",r#"(()=>{
      const icon=document.getElementById('icon'),shape=document.getElementById('shape');
      let hooks=0;Object.defineProperty(icon,'namespaceURI',{get(){hooks++;throw Error('public namespace shadow')}});
      Object.defineProperty(icon,'getAttribute',{value(){hooks++;throw Error('public attribute shadow')}});
      const first=Element.prototype.getBoundingClientRect.call(icon);
      let unsupported='';try{shape.getBoundingClientRect()}catch(error){unsupported=error.message}
      icon.setAttribute('width','40');const second=Element.prototype.getBoundingClientRect.call(icon);
      return String(first.x===0&&first.y===0&&first.width===64.5078125&&first.height===21.5078125&&second.width===40&&hooks===0&&unsupported.includes('SVG descendant'));
    })()"#).unwrap();
    assert_eq!(actual, "true");
}

#[cfg(target_os = "macos")]
#[test]
fn mixed_node_range_wrapping_scroll_and_caret_hits_match_independent_chrome() {
    let probe = include_str!("../../../../harness/detection/range-flow-geometry-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/range-flow-geometry-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn ordinary_rotated_div_and_image_geometry_origins_hits_and_carets_match_chrome() {
    let probe = include_str!("../../../../harness/detection/affine-element-geometry-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/affine-element-geometry-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}

#[test]
fn original_utf16_boundaries_survive_scalar_shaping_and_private_range_caret_lifecycle() {
    let probe = include_str!("../../../../harness/detection/utf16-text-geometry-probe.js")
        .trim()
        .trim_end_matches(';');
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/utf16-text-geometry-chrome155.json"
    ))
    .unwrap();
    let actual = run_with_dom(
        "<!doctype html><html><head></head><body></body></html>",
        &format!("JSON.stringify({probe})"),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
        expected["result"]
    );
}
