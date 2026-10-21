(() => {
  const rect = value => [value.x,value.y,value.width,value.height];
  const rows = {};
  for (const id of ['link','heading']) {
    const node = document.getElementById(id), range = document.createRange();
    range.selectNodeContents(node);
    const computed = getComputedStyle(node);
    rows[id] = {
      bounds:rect(node.getBoundingClientRect()),
      rects:Array.from(node.getClientRects(),rect),
      contentsRange:{bounds:rect(range.getBoundingClientRect()),rects:Array.from(range.getClientRects(),rect)},
      style:Object.fromEntries(['fontFamily','fontSize','fontWeight','fontStyle','lineHeight'].map(key=>[key,computed[key]]))
    };
  }
  return JSON.stringify({rows});
})()
