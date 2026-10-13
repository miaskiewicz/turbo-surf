//! User Timing and navigation phase contracts are deterministic and offline.
use turbo_surf_render::{run_with_dom, PageSession};

#[test]
fn user_timing_keeps_real_entries_and_cloned_detail() {
    let result=run_with_dom("<body></body>",r#"(() => {
      performance.clearMarks();performance.clearMeasures();
      const source={value:1},a=performance.mark('a',{startTime:0,detail:source});source.value=9;
      performance.mark('a',{startTime:5});performance.mark('b',{startTime:12});
      const measure=performance.measure('delta','a','b');
      const duration=performance.measure('duration',{start:3,duration:4,detail:0});
      const detached=new PerformanceMark('detached',{startTime:2});
      const selected=performance.getEntriesByName('a','mark');selected.pop();
      const before=[performance.getEntriesByType('mark').length,performance.getEntriesByType('measure').length];
      performance.clearMarks('a');performance.clearMeasures('delta');
      return JSON.stringify({a:[a.startTime,a.detail.value,a instanceof PerformanceMark,a instanceof PerformanceEntry,Object.keys(a).length],
        measure:[measure.startTime,measure.duration,measure instanceof PerformanceMeasure],duration:duration.toJSON(),before,
        navigation:[Number.isInteger(a.navigationId),a.navigationId>=100&&a.navigationId<=10000,measure.navigationId===a.navigationId,detached.navigationId===a.navigationId,performance.getEntriesByType('navigation')[0].navigationId===a.navigationId,duration.toJSON().navigationId===a.navigationId],detached:performance.getEntriesByName('detached').length,remaining:[performance.getEntriesByType('mark').map(x=>x.name),performance.getEntriesByType('measure').map(x=>x.name)],
        readonly:(()=>{a.name='bad';return a.name==='a';})(),tag:Object.prototype.toString.call(measure)});
    })()"#).unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(v["a"], serde_json::json!([0, 1, true, true, 0]));
    assert_eq!(v["measure"], serde_json::json!([5, 7, true]));
    assert_eq!(
        v["navigation"],
        serde_json::json!([true, true, true, true, true, true])
    );
    let mut duration = v["duration"].clone();
    duration.as_object_mut().unwrap().remove("navigationId");
    assert_eq!(
        duration,
        serde_json::json!({"name":"duration","entryType":"measure","startTime":3,"duration":4})
    );
    assert_eq!(v["before"], serde_json::json!([3, 2]));
    assert_eq!(v["detached"], 0);
    assert_eq!(v["remaining"], serde_json::json!([["b"], ["duration"]]));
    assert_eq!(v["readonly"], true);
    assert_eq!(v["tag"], "[object PerformanceMeasure]");
}

#[test]
fn user_timing_rejects_invalid_inputs_and_preserves_the_buffer() {
    let result=run_with_dom("<body></body>",r#"(() => {
      const errors=[];for(const fn of [()=>performance.mark(),()=>performance.mark('bad',{startTime:-1}),()=>performance.mark('bad',{startTime:NaN}),()=>performance.mark('navigationStart'),()=>performance.mark('uncloneable',{detail:()=>{}}),()=>performance.mark('getter',{detail:{get bad(){throw new RangeError('preserve');}}}),()=>performance.measure('missing','none'),()=>performance.measure('bad',{start:1,end:2,duration:1}),()=>performance.measure('bad',{detail:1}),()=>performance.getEntriesByName(),()=>performance.getEntriesByType(),()=>Performance.prototype.mark.call({},'x')]){try{fn();errors.push('accepted');}catch(e){errors.push(e.name);}}
      performance.clearMarks();performance.clearMeasures();
      const handler=()=>{};performance.onresourcetimingbufferfull=handler;
      const counts=performance.eventCounts;let invalid=false;try{EventCounts.prototype.get.call({},'click');}catch(e){invalid=e instanceof TypeError;}
      return JSON.stringify({errors,handler:performance.onresourcetimingbufferfull===handler,counts:[counts.size,counts.get('click'),counts.has('click'),Array.from(counts).length],names:[counts.has('gotpointercapture'),counts.has('lostpointercapture'),counts.has('drag'),counts.has('mousemove')],invalid,interaction:performance.interactionCount,toJSON:performance.toJSON().timeOrigin===performance.timeOrigin,entries:performance.getEntries().length});
    })()"#).unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        v["errors"],
        serde_json::json!([
            "TypeError",
            "TypeError",
            "TypeError",
            "SyntaxError",
            "DataCloneError",
            "RangeError",
            "SyntaxError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError",
            "TypeError"
        ])
    );
    assert_eq!(v["counts"], serde_json::json!([36, 0, true, 36]));
    assert_eq!(v["names"], serde_json::json!([true, true, false, false]));
    for key in ["handler", "invalid", "toJSON"] {
        assert_eq!(v[key], true, "{v}");
    }
    assert_eq!(v["interaction"], 0);
    assert_eq!(v["entries"], 1);
}

#[tokio::test]
async fn navigation_timing_records_actual_lifecycle_boundaries() {
    let mut page=PageSession::open(r#"<script>
      globalThis.phases=[];
      document.addEventListener('DOMContentLoaded',()=>phases.push(['dcl',performance.timing.domInteractive>0,performance.timing.domContentLoadedEventStart>0,performance.timing.domContentLoadedEventEnd===0,performance.timing.loadEventStart===0]));
      window.addEventListener('load',()=>phases.push(['load',performance.timing.domComplete>0,performance.timing.loadEventStart>0,performance.timing.loadEventEnd===0]));
    </script><body></body>"#,"http://localhost/page","","",5000).await.unwrap();
    let result=page.eval(r#"globalThis.__RESULT=JSON.stringify({phases,nav:performance.getEntriesByType('navigation')[0].toJSON(),legacy:performance.timing.toJSON(),now:performance.now(),name:performance.getEntriesByType('navigation')[0].name})"#).await.unwrap();
    let v: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        v["phases"],
        serde_json::json!([["dcl", true, true, true, true], ["load", true, true, true]])
    );
    assert_eq!(v["name"], "http://localhost/page");
    for key in [
        "domainLookupStart",
        "domainLookupEnd",
        "connectStart",
        "connectEnd",
        "secureConnectionStart",
        "requestStart",
        "responseStart",
    ] {
        assert_eq!(v["legacy"][key], 0);
    }
    let keys = [
        "navigationStart",
        "domLoading",
        "domInteractive",
        "domContentLoadedEventStart",
        "domContentLoadedEventEnd",
        "domComplete",
        "loadEventStart",
        "loadEventEnd",
    ];
    let mut previous = 0.;
    for key in keys {
        let n = v["legacy"][key].as_f64().unwrap();
        assert!(n >= previous, "{v}");
        previous = n;
    }
    assert!(v["nav"]["loadEventEnd"].as_f64().unwrap() <= v["now"].as_f64().unwrap() + 1.);
}
