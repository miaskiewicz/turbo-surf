(() => {
  document.body.style.cssText = "margin:0;height:300px;position:relative";
  const cases = [
    [
      "overlap",
      "<div id=lower style='position:absolute;left:10px;top:10px;width:80px;height:80px;z-index:1'></div><div id=upper style='position:absolute;left:30px;top:30px;width:80px;height:80px;z-index:2'></div>",
      [
        [20, 20],
        [40, 40],
        [105, 105],
        [120, 120],
      ],
    ],
    [
      "pointer",
      "<div id=lower style='position:absolute;left:10px;top:10px;width:80px;height:80px'></div><div id=upper style='position:absolute;left:10px;top:10px;width:80px;height:80px;pointer-events:none'></div>",
      [
        [20, 20],
        [100, 100],
      ],
    ],
    [
      "clip",
      "<div id=clip style='position:absolute;left:10px;top:10px;width:40px;height:40px;overflow:hidden'><div id=child style='width:80px;height:80px'></div></div>",
      [
        [20, 20],
        [60, 20],
        [20, 60],
      ],
    ],
    [
      "rotate",
      "<div id=rotated style='position:absolute;left:100px;top:100px;width:40px;height:20px;transform:rotate(90deg);transform-origin:0 0'></div>",
      [
        [90, 110],
        [105, 110],
        [81, 139],
        [80, 140],
      ],
    ],
    [
      "visibility",
      "<div id=hidden style='position:absolute;left:10px;top:10px;width:80px;height:80px;visibility:hidden'><div id=visible style='width:20px;height:20px;visibility:visible'></div></div>",
      [
        [15, 15],
        [50, 50],
      ],
    ],
  ];
  const results = [];
  const name = (node) => node?.id || node?.localName || null;
  for (const [fixture, html, points] of cases) {
    document.body.innerHTML = html;
    results.push({
      fixture,
      points: points.map(([x, y]) => ({
        point: [x, y],
        one: name(document.elementFromPoint(x, y)),
        all: document.elementsFromPoint(x, y).map(name),
      })),
    });
  }
  return results;
})();
