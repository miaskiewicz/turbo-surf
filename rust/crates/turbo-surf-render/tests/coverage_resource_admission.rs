//! Real resource admission contrasts: live/revoked bytes and bounded CSP source samples.
use serde_json::{json, Value};
use turbo_surf_render::PageSession;

#[tokio::test(flavor = "current_thread")]
async fn blob_image_admits_original_bytes_but_revocation_denies_new_consumers() {
    let mut page = PageSession::open("<body>", "https://blob-image.test/", "", "", 5000)
        .await
        .unwrap();
    let value: Value = serde_json::from_str(&page.eval_json_expression(r#"(async()=>{
      const bytes='<svg xmlns="http://www.w3.org/2000/svg" width="13" height="7"></svg>';
      const url=URL.createObjectURL(new Blob([bytes],{type:'image/svg+xml'}));
      const image=new Image();document.body.append(image);image.src=url;await image.decode();
      const before=[image.naturalWidth,image.naturalHeight,image.complete];
      URL.revokeObjectURL(url);
      const next=new Image();document.body.append(next);next.src=url;
      let rejected=false;try{await next.decode();}catch{rejected=true;}
      return {before,retained:[image.naturalWidth,image.naturalHeight],after:[next.naturalWidth,next.naturalHeight],rejected};
    })()"#).await.unwrap()).unwrap();
    assert_eq!(
        value,
        json!({"before":[13,7,true],"retained":[13,7],"after":[0,0],"rejected":true})
    );
}

#[tokio::test(flavor = "current_thread")]
async fn csp_resource_sample_trims_long_padding_and_limits_utf16_code_units() {
    let headers = vec![(
        "Content-Security-Policy".into(),
        "script-src 'none' 'report-sample'".into(),
    )];
    let mut page = PageSession::open_with_response_headers(
        "<body>",
        "https://sample-resource.test/",
        "",
        "",
        5000,
        "",
        Default::default(),
        &headers,
    )
    .await
    .unwrap();
    let value: Value = serde_json::from_str(&page.eval_json_expression(r#"(async()=>{
      const samples=[];document.addEventListener('securitypolicyviolation',event=>samples.push({sample:event.sample,blocked:event.blockedURI,directive:event.effectiveDirective}));
      const source=' '.repeat(260)+'/*😀😀*/globalThis.forbiddenSampleRun=true;/*'+'x'.repeat(60)+'*/'+'\t'.repeat(260);
      let rejected=false;try{eval(source);}catch(error){rejected=error instanceof EvalError;}
      const script=document.createElement('script');script.textContent=source;document.body.append(script);
      await new Promise(resolve=>setTimeout(resolve,0));
      return {samples,rejected,ran:globalThis.forbiddenSampleRun===true,expected:source.trim().slice(0,40)};
    })()"#).await.unwrap()).unwrap();
    assert_eq!(value["rejected"], true);
    assert_eq!(value["ran"], false);
    assert_eq!(
        value["samples"],
        json!([
            {"sample":"","blocked":"eval","directive":"script-src"},
            {"sample":value["expected"].clone(),"blocked":"inline","directive":"script-src-elem"}
        ])
    );
    assert_eq!(
        value["expected"].as_str().unwrap().encode_utf16().count(),
        40
    );
}
