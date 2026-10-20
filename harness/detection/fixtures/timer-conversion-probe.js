(async () => {
  const start = performance.now();
  const cases = [["zero",0],["huge-finite",1e300],["infinity",Infinity],["nan",NaN],["int32-max",2**31-1],["int32-overflow",2**31],["int32-overflow-plus-one",2**31+1],["uint32-wrap-five",2**32+5],["negative-one",-1]];
  const rows = [], ids = [], fireOrder = [];
  for (const kind of ["timeout","interval"]) {
    for (const [label, delay] of cases) {
      const row = { kind, label, fired: false, elapsedMs: null, error: null };
      rows.push(row);
      let id;
      try {
        const callback = () => {
          row.fired = true;
          row.elapsedMs = performance.now() - start;
          fireOrder.push(kind + ":" + label);
          if (kind === "interval") clearInterval(id);
        };
        id = kind === "timeout" ? setTimeout(callback,delay) : setInterval(callback,delay);
        ids.push([kind,id]);
      } catch (error) { row.error = error.name; }
    }
  }
  const immediateFired = rows.filter(row => row.fired).length;
  await new Promise(resolve => setTimeout(resolve,200));
  for (const [kind,id] of ids) (kind === "timeout" ? clearTimeout : clearInterval)(id);
  return JSON.stringify({observationMs:200, elapsedMs:performance.now()-start, immediateFired, rows, fireOrder});
})()
