(async () => {
  const rows = [];
  for (const opaque of [false, true]) {
    const frame = document.createElement("iframe");
    if (opaque) frame.setAttribute("sandbox", "allow-scripts");
    const row = await new Promise((resolve) => {
      const timeout = setTimeout(() => {
        window.removeEventListener("message", receive);
        resolve({ timeout: true });
      }, 2500);
      function receive(event) {
        clearTimeout(timeout);
        window.removeEventListener("message", receive);
        resolve({ ...event.data, sourceMatches: event.source === frame.contentWindow });
      }
      window.addEventListener("message", receive);
      frame.srcdoc = `<!doctype html><head><base href="https://base.example/initial/"></head><body><script>
        let row;try {
          const base=document.querySelector('base');
          const descriptor=Object.getOwnPropertyDescriptor(location,'href');
          row={before:document.baseURI,href:location.href,getType:typeof descriptor.get,getName:descriptor.get.name};
          Element.prototype.getAttribute=()=>{throw new Error('guest attribute')};
          Document.prototype.querySelectorAll=()=>{throw new Error('guest query')};
          base.href='https://base.example/updated/';row.after=document.baseURI;
        }catch(error){row={error:error.name};}
        parent.postMessage(row,'*');
      </script>`;
      document.body.append(frame);
    });
    rows.push({ opaque, row });
    frame.remove();
  }
  return rows;
})();
