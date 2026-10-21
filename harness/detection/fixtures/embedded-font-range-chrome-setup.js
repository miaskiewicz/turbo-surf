(async () => {
  const response=await fetch('/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf');
  if(!response.ok)throw Error('Embedded font fetch failed: '+response.status);
  const bytes=await response.arrayBuffer();
  const face=new FontFace('IBM Plex Mono',bytes,{style:'normal',weight:'400'});
  await face.load();document.fonts.add(face);await document.fonts.ready;
  const sha256=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),value=>value.toString(16).padStart(2,'0')).join('');
  return JSON.stringify({fontFamily:face.family,status:face.status,sha256,byteLength:bytes.byteLength});
})()
