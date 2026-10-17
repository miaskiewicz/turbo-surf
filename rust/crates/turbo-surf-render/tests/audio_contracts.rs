//! Real offline graph/ownership/lifecycle contracts and independent Chrome PCM.
use serde_json::{json, Value};
async fn evaluate(source: &str) -> Value {
    let source = format!("(async()=>{{{source}}})()");
    let result = turbo_surf_render::eval_async(
        "<!doctype html><body></body>",
        "http://audio.test/",
        &source,
    )
    .await
    .unwrap();
    serde_json::from_str(&result).unwrap_or_else(|_| panic!("invalid audio result: {result}"))
}

#[tokio::test(flavor = "current_thread")]
async fn offline_audio_matches_independent_chrome_graphs_and_error_contracts() {
    let collector = include_str!("../../../../harness/detection/audio-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = evaluate(&format!(
        "globalThis.__RESULT=JSON.stringify(await {collector});"
    ))
    .await;
    for (name, hash) in [
        ("triangle", "9f58dfe3"),
        ("compressor", "a8d73295"),
        ("fingerprint", "63b6b305"),
        ("sine", "fdca1b60"),
        ("squareGain", "2b8170b3"),
        ("alternateCompression", "17d003f4"),
    ] {
        assert!(
            value["graphs"][name]["error"].is_null(),
            "{name}: {}",
            value["graphs"][name]
        );
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        assert_eq!(value["graphs"][name]["hash"], hash, "{name}");
        #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
        let _ = hash;
        let graph = &value["graphs"][name];
        assert!(graph["energy"].as_f64().unwrap() > 1000.0);
        assert_eq!(graph["samples"], 44100);
        assert_eq!(graph["stateBefore"], "suspended");
        assert_eq!(graph["stateAfter"], "closed");
        assert_eq!(graph["stableChannelView"], true);
        assert_eq!(graph["sourceContext"], true);
        assert_eq!(graph["promiseBrand"], "[object Promise]");
        assert_eq!(graph["currentTime"], 44160.0 / 44100.0);
    }
    for (name, error) in [
        ("withoutNew", "TypeError"),
        ("zeroChannels", "NotSupportedError"),
        ("zeroLength", "NotSupportedError"),
        ("lowSampleRate", "NotSupportedError"),
        ("foreignBufferReceiver", "TypeError"),
        ("repeatedStart", "InvalidStateError"),
        ("crossContextConnection", "InvalidAccessError"),
    ] {
        assert_eq!(
            value["contracts"][name]["error"]["name"], error,
            "{name}: {}",
            value["contracts"][name]
        );
    }
    assert_eq!(value["contracts"]["withoutNew"]["error"]["message"],"Failed to construct 'OfflineAudioContext': Please use the 'new' operator, this DOM object constructor cannot be called as a function.");
}

#[tokio::test(flavor = "current_thread")]
async fn native_audio_buffers_are_owned_mutable_branded_and_cannot_be_forged_or_cloned() {
    let value=evaluate(r#"
      const context=new OfflineAudioContext(2,256,44100),buffer=context.createBuffer(2,256,44100),source=context.createBufferSource(),gain=context.createGain();
      const input=new Float32Array([.25,.5,-.25]);buffer.copyToChannel(input,0,5);const copied=new Float32Array(3);buffer.copyFromChannel(copied,0,5);
      source.buffer=buffer;source.connect(gain);gain.gain.value=2;gain.connect(context.destination);source.disconnect();source.connect(gain);source.start();
      const output=await context.startRendering();let illegal=0,cloneError;
      for(const receiver of [{},Object.create(AudioBuffer.prototype)])try{AudioBuffer.prototype.getChannelData.call(receiver,0);}catch(e){if(e instanceof TypeError)illegal++;}
      try{structuredClone(buffer);}catch(e){cloneError=e.name;}
      const ownBefore=Object.keys(buffer);Object.defineProperty(buffer,'numberOfChannels',{value:99});
      globalThis.__RESULT=JSON.stringify({output:Array.from(output.getChannelData(0).slice(5,8)),copied:Array.from(copied),secondSilent:output.getChannelData(1).every(value=>value===0),stable:buffer.getChannelData(0)===buffer.getChannelData(0),brand:Object.prototype.toString.call(buffer),owned:Object.getPrototypeOf(output)===AudioBuffer.prototype,illegal,cloneError,ownBefore,native:Function.prototype.toString.call(AudioNode.prototype.connect).includes('[native code]'),stateHidden:typeof Deno==='undefined'&&typeof __nativeAudioRecord==='undefined'});
    "#).await;
    assert_eq!(value["output"], json!([0.5, 1, -0.5]));
    assert_eq!(value["copied"], json!([0.25, 0.5, -0.25]));
    for name in ["secondSilent", "stable", "owned", "native", "stateHidden"] {
        assert_eq!(value[name], true, "{value}");
    }
    assert_eq!(value["brand"], "[object AudioBuffer]");
    assert_eq!(value["illegal"], 2);
    assert_eq!(value["cloneError"], "DataCloneError");
    assert_eq!(value["ownBefore"], json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn native_audio_completion_dispatches_real_trusted_events_and_closed_state() {
    let value=evaluate(r#"
      const context=new OfflineAudioContext(1,128,44100);let completion,states=[];
      context.addEventListener('statechange',event=>states.push([context.state,event.isTrusted]));
      context.oncomplete=event=>{completion={trusted:event.isTrusted,target:event.target===context,brand:Object.prototype.toString.call(event),buffer:event.renderedBuffer};};
      const call=context.startRendering(),output=await call;
      globalThis.__RESULT=JSON.stringify({states,promise:call instanceof Promise,completion:completion&&{trusted:completion.trusted,target:completion.target,brand:completion.brand,sameBuffer:completion.buffer===output},silence:output.getChannelData(0).every(value=>value===0)});
    "#).await;
    assert_eq!(value["promise"], true);
    assert_eq!(value["silence"], true);
    assert_eq!(
        value["completion"],
        json!({"trusted":true,"target":true,"brand":"[object OfflineAudioCompletionEvent]","sameBuffer":true})
    );
    assert!(value["states"]
        .as_array()
        .unwrap()
        .contains(&json!(["closed", true])));
}

#[tokio::test(flavor = "current_thread")]
async fn offline_audio_limits_fail_explicitly_and_never_substitute_samples() {
    let value=evaluate(r#"
      const error=async callback=>{try{await callback();return null;}catch(e){return{name:e.name,dom:e instanceof DOMException};}};
      const cyclic=new OfflineAudioContext(1,128,44100),a=cyclic.createGain(),b=cyclic.createGain();a.connect(b);b.connect(a);a.connect(cyclic.destination);
      const rate=new OfflineAudioContext(1,128,44100),source=rate.createBufferSource();source.playbackRate.value=2;source.connect(rate.destination);source.start();
      const context=new OfflineAudioContext(1,128,44100),oscillator=context.createOscillator(),compressor=context.createDynamicsCompressor();
      globalThis.__RESULT=JSON.stringify({cycle:await error(()=>cyclic.startRendering()),rate:await error(()=>rate.startRendering()),rateState:rate.state,frames:await error(()=>new OfflineAudioContext(2,200000,44100)),automation:await error(()=>oscillator.frequency.linearRampToValueAtTime(880,1)),delayedStart:await error(()=>oscillator.start(.1)),live:await error(()=>new AudioContext()),paramRates:[oscillator.frequency.automationRate,compressor.threshold.automationRate,source.playbackRate.automationRate],forgedCtor:await error(()=>OfflineAudioContext.call(Object.create(OfflineAudioContext.prototype),1,128,44100))});
    "#).await;
    for name in ["cycle", "live"] {
        assert_eq!(
            value[name],
            json!({"name":"NotSupportedError","dom":true}),
            "{value}"
        );
    }
    for name in ["rate", "automation", "delayedStart"] {
        assert_eq!(value[name], Value::Null, "{value}");
    }
    assert_eq!(value["frames"]["name"], "QuotaExceededError");
    assert_eq!(value["forgedCtor"]["name"], "TypeError");
    assert_eq!(value["rateState"], "closed");
    assert_eq!(value["paramRates"], json!(["a-rate", "k-rate", "k-rate"]));
}

#[tokio::test(flavor = "current_thread")]
async fn borrowed_audio_methods_preserve_context_and_method_realm_ownership() {
    let value=evaluate(r#"
      const frame=document.createElement('iframe');frame.srcdoc='<!doctype html><body>audio child</body>';document.body.appendChild(frame);
      await new Promise(resolve=>setTimeout(resolve,10));const child=frame.contentWindow;
      const context=new child.OfflineAudioContext(1,128,44100),buffer=BaseAudioContext.prototype.createBuffer.call(context,1,128,44100);
      const foreign=child.BaseAudioContext.prototype.createBuffer.call(context,1,128,44100);
      const view=AudioBuffer.prototype.getChannelData.call(foreign,0),source=BaseAudioContext.prototype.createBufferSource.call(context);
      foreign.getChannelData(0).fill(.25);source.buffer=foreign;AudioNode.prototype.connect.call(source,context.destination);AudioScheduledSourceNode.prototype.start.call(source);
      const rendered=await OfflineAudioContext.prototype.startRendering.call(context);
      globalThis.__RESULT=JSON.stringify({parentBuffer:Object.getPrototypeOf(buffer)===AudioBuffer.prototype,foreignBuffer:Object.getPrototypeOf(foreign)===child.AudioBuffer.prototype,borrowedView:Object.getPrototypeOf(view)===child.Float32Array.prototype,sourceRealm:Object.getPrototypeOf(source)===AudioBufferSourceNode.prototype,context:source.context===context,renderedRealm:Object.getPrototypeOf(rendered)===AudioBuffer.prototype,pcm:rendered.getChannelData(0).every(value=>value===.25),childConstructor:child.OfflineAudioContext!==OfflineAudioContext,hidden:typeof child.Deno==='undefined'});
    "#).await;
    for name in [
        "parentBuffer",
        "foreignBuffer",
        "borrowedView",
        "sourceRealm",
        "context",
        "renderedRealm",
        "pcm",
        "childConstructor",
        "hidden",
    ] {
        assert_eq!(value[name], true, "{name}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn private_audio_state_bypasses_guest_shadows_iterators_and_handles_detached_channels() {
    let probe = include_str!("../../../../harness/detection/audio-ownership-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = evaluate(&format!(
        "globalThis.__RESULT=JSON.stringify(await ({probe}));"
    ))
    .await;
    assert_eq!(value["shadowResult"], 0.5);
    assert_eq!(value["iteratorResult"], 0.25);
    assert_eq!(value["transferredPCM"], 0);
    assert_eq!(value["transferError"], Value::Null);
    assert_eq!(value["transferredResult"], Value::Null);
    assert_eq!(value["copyPast"], Value::Null);
    assert_eq!(value["channelAfterTransferLength"], 0);
    assert_eq!(value["originalAfterTransferLength"], 0);
    assert_eq!(
        value["events"],
        json!([
            ["state", true, "running"],
            ["ended", true, "running"],
            [
                "complete",
                true,
                "closed",
                "[object OfflineAudioCompletionEvent]"
            ],
            ["state", true, "closed"]
        ])
    );
    assert_eq!(
        value["params"],
        json!({"compressor":"k-rate","buffer":"k-rate","osc":"a-rate"})
    );
    assert_eq!(value["webkitOffline"], "undefined");
}

#[tokio::test(flavor = "current_thread")]
async fn native_audio_parameter_metadata_and_constructor_dictionaries_keep_real_state() {
    let value=evaluate(r#"
      const context=new OfflineAudioContext(1,44100,44100),compressor=context.createDynamicsCompressor();
      let conversions=0;const oscillator=new OscillatorNode(context,{get frequency(){conversions++;return 880;}}),gain=new GainNode(context,null);
      const defaults={attack:compressor.attack.value,defaultAttack:compressor.attack.defaultValue,rate:compressor.attack.automationRate,arity:AudioParam.prototype.setTargetAtTime.length,reduction:compressor.reduction};
      compressor.threshold.value=-50;compressor.knee.value=40;compressor.ratio.value=12;compressor.attack.value=0;compressor.release.value=.25;
      oscillator.connect(compressor);compressor.connect(context.destination);oscillator.start();await context.startRendering();
      let wrongBrand;try{Object.getOwnPropertyDescriptor(BaseAudioContext.prototype,'onstatechange').get.call(context.createBuffer(1,128,44100));}catch(e){wrongBrand=e.name;}
      globalThis.__RESULT=JSON.stringify({defaults,conversions,frequency:oscillator.frequency.value,gain:gain.gain.value,reduction:compressor.reduction,reductionNative:Function.prototype.toString.call(Object.getOwnPropertyDescriptor(DynamicsCompressorNode.prototype,'reduction').get).includes('[native code]'),wrongBrand});
    "#).await;
    assert_eq!(value["defaults"]["attack"], f64::from(0.003f32));
    assert_eq!(value["defaults"]["defaultAttack"], f64::from(0.003f32));
    assert_eq!(value["defaults"]["rate"], "k-rate");
    assert_eq!(value["defaults"]["arity"], 3);
    assert_eq!(value["defaults"]["reduction"], 0);
    assert_eq!(value["conversions"], 1);
    assert_eq!(value["frequency"], 880);
    assert_eq!(value["gain"], 1);
    assert!(value["reduction"].as_f64().unwrap() < 0.);
    assert_eq!(value["reductionNative"], true);
    assert_eq!(value["wrongBrand"], "TypeError");
}

#[tokio::test(flavor = "current_thread")]
async fn advanced_audio_sources_automation_and_effects_match_independent_chrome_controls() {
    let collector = include_str!("../../../../harness/detection/audio-advanced-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = evaluate(&format!(
        "globalThis.__RESULT=JSON.stringify(await {collector});"
    ))
    .await;
    for (name, hash) in [
        ("delayedOscillator", "5bd8d8b0"),
        ("gainRamp", "102d129b"),
        ("exponentialRamp", "f316f0ec"),
        ("target", "c7b76bb4"),
        ("curve", "6b7768c5"),
        ("frequencyRamp", "75d63da6"),
        ("detunedOscillator", "d175c454"),
        ("offsetDuration", "52d2a640"),
        ("doubleRate", "8115090d"),
        ("halfRate", "9131fe07"),
        ("detunedBuffer", "4e964469"),
        ("loop", "0bec9cb2"),
        ("resample", "9095e4dd"),
        ("lowpass", "e41add11"),
        ("highpass", "daf19440"),
        ("waveshaper", "4f67288c"),
        ("delay", "4bdfb7c0"),
        ("pan", "7c4f8279"),
    ] {
        let graph = &value["graphs"][name];
        assert!(graph["error"].is_null(), "{name}: {graph}");
        assert!(graph["energy"].as_f64().unwrap() > 1., "{name}");
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        assert_eq!(graph["hash"], hash, "{name}");
        #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
        let _ = hash;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn advanced_audio_interfaces_preserve_private_brands_rate_and_curve_contracts() {
    let value=evaluate(r#"
 const c=new OfflineAudioContext(1,2048,44100),s=c.createBufferSource(),g=c.createGain(),b=c.createBiquadFilter(),w=c.createWaveShaper(),d=c.createDelay(),p=c.createStereoPanner();
 const err=fn=>{try{fn();return null;}catch(error){return error.name;}};
 const errors={bufferRate:err(()=>s.playbackRate.automationRate='a-rate'),compRate:err(()=>c.createDynamicsCompressor().threshold.automationRate='a-rate'),curveShort:err(()=>g.gain.setValueCurveAtTime([1],0,.1)),curveNonfinite:err(()=>g.gain.setValueCurveAtTime([1,NaN],0,.1)),plainShaper:err(()=>w.curve=[0,1]),delayZero:err(()=>c.createDelay(0)),overlap:err(()=>{g.gain.setValueCurveAtTime([0,1],0,.1);g.gain.setValueAtTime(.2,.01);})};
 const curve=new Float32Array([-1,0,1]);w.curve=curve;curve[0]=99;
 const buffer=c.createBuffer(1,128,44100);s.buffer=buffer;errors.bufferReset=err(()=>s.buffer=buffer);
 let brands=0;for(const node of [b,w,d,p])if(Object.prototype.toString.call(node)==='[object '+node.constructor.name+']'&&node.context===c)brands++;
 globalThis.__RESULT=JSON.stringify({errors,brands,gainMaximum:b.gain.maxValue,curveCopied:w.curve!==curve&&w.curve[0]===-1,native:Function.prototype.toString.call(BaseAudioContext.prototype.createBiquadFilter).includes('[native code]'),private:typeof Deno==='undefined',sameReturn:g.gain.cancelScheduledValues(0)===g.gain});
 "#).await;
    assert_eq!(value["brands"], 4);
    assert_eq!(value["gainMaximum"], 1541.273681640625);
    for field in ["curveCopied", "native", "private", "sameReturn"] {
        assert_eq!(value[field], true, "{value}");
    }
    for (name, expected) in [
        ("bufferRate", "InvalidStateError"),
        ("compRate", "InvalidStateError"),
        ("curveShort", "InvalidStateError"),
        ("curveNonfinite", "TypeError"),
        ("plainShaper", "TypeError"),
        ("delayZero", "NotSupportedError"),
        ("overlap", "NotSupportedError"),
        ("bufferReset", "InvalidStateError"),
    ] {
        assert_eq!(value["errors"][name], expected, "{value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn audio_cancel_hold_rate_reverse_playback_and_error_controls_match_chrome() {
    let collector = include_str!("../../../../harness/detection/audio-advanced-contract-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = evaluate(&format!(
        "globalThis.__RESULT=JSON.stringify(await {collector});"
    ))
    .await;
    for (name, error) in [
        ("bufferARate", Some("InvalidStateError")),
        ("compressorARate", Some("InvalidStateError")),
        ("oscKRate", None),
        ("zeroExp", Some("RangeError")),
        ("shortCurve", Some("InvalidStateError")),
        ("emptyCurve", Some("InvalidStateError")),
        ("curveNonfinite", Some("TypeError")),
        ("curveOverlap", Some("NotSupportedError")),
        ("bufferDoubleSet", Some("InvalidStateError")),
        ("delayMax0", Some("NotSupportedError")),
        ("shaperPlainArray", Some("TypeError")),
    ] {
        match error {
            Some(error) => assert_eq!(value["errors"][name]["name"], error, "{name}: {value}"),
            None => assert_eq!(value["errors"][name], Value::Null, "{name}: {value}"),
        };
    }
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/audio-advanced-chrome155.json"
    ))
    .unwrap();
    for (name, graph) in value["graphs"].as_object().unwrap() {
        assert!(graph["error"].is_null(), "{name}: {graph}");
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        {
            use sha2::{Digest, Sha256};
            let mut hash = Sha256::new();
            for value in graph["pcm"].as_array().unwrap() {
                hash.update((value.as_f64().unwrap() as f32).to_le_bytes());
            }
            let expected = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["name"] == name.as_str())
                .unwrap();
            assert_eq!(
                format!("{:x}", hash.finalize()),
                expected["sha256"][0].as_str().unwrap(),
                "{name}"
            );
        }
    }
    #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
    let _ = fixture;
}

#[tokio::test(flavor = "current_thread")]
async fn biquad_response_has_real_filter_math_alias_order_brands_and_errors() {
    let collector = include_str!("../../../../harness/detection/audio-effects-response-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = evaluate(&format!(
        "globalThis.__RESULT=JSON.stringify(await {collector});"
    ))
    .await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/audio-effects-frequency-chrome155.json"
    ))
    .unwrap();
    for field in ["errors", "shape"] {
        assert_eq!(value[field], fixture["result"][field], "{field}: {value}");
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    for field in ["graphs", "overlap", "same"] {
        assert_eq!(value[field], fixture["result"][field], "{field}: {value}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn low_reverse_and_scheduled_oscillator_parameter_quantums_match_chrome() {
    let collector = include_str!("../../../../harness/detection/audio-oscillator-corners-probe.js")
        .trim()
        .trim_end_matches(';');
    let value = evaluate(&format!(
        "globalThis.__RESULT=JSON.stringify(await {collector});"
    ))
    .await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../harness/detection/fixtures/audio-advanced-chrome155.json"
    ))
    .unwrap();
    for (name, graph) in value.as_object().unwrap() {
        assert!(graph["error"].is_null(), "{name}: {graph}");
        #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
        {
            use sha2::{Digest, Sha256};
            let mut hash = Sha256::new();
            for value in graph["pcm"].as_array().unwrap() {
                hash.update((value.as_f64().unwrap() as f32).to_le_bytes());
            }
            let expected = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["name"] == format!("oscillator_{name}"))
                .unwrap();
            assert_eq!(
                format!("{:x}", hash.finalize()),
                expected["sha256"][0].as_str().unwrap(),
                "{name}"
            );
        }
    }
    #[cfg(not(all(target_arch = "aarch64", target_os = "macos")))]
    let _ = fixture;
}

#[tokio::test(flavor = "current_thread")]
async fn native_audio_private_automation_and_graph_records_do_not_call_guest_index_hooks() {
    let value=evaluate(r#"
 const c=new OfflineAudioContext(1,128,44100),s=c.createBufferSource(),b=c.createBuffer(1,128,44100),g=c.createGain();b.getChannelData(0).fill(.5);s.buffer=b;s.connect(g);g.connect(c.destination);
 let hooks=0;const old=Object.getOwnPropertyDescriptor(Array.prototype,'0');Object.defineProperty(Array.prototype,'0',{configurable:true,get(){hooks++;return 0;},set(value){hooks++;Object.defineProperty(this,'0',{value,writable:true,enumerable:true,configurable:true});}});
 let output,error;
 try{g.gain.setValueAtTime(.5,0);g.gain.linearRampToValueAtTime(1,.01);s.start();output=await c.startRendering();}catch(caught){error=caught.name;}
 finally{if(old)Object.defineProperty(Array.prototype,'0',old);else delete Array.prototype[0];}
 globalThis.__RESULT=JSON.stringify({hooks,error:error??null,sample:output?.getChannelData(0)[0],closed:c.state==='closed'});
 "#).await;
    assert_eq!(
        value,
        json!({"hooks":0,"error":null,"sample":0.25,"closed":true})
    );
}
