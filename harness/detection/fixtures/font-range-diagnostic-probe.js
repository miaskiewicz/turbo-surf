(() => {
  document.body.style.fontFamily="serif";
  const ranges=[];
  for(const text of ["Old","result","Old result"]){
    for(const size of ["18.72px","18.71875px"]){
      const node=document.createElement("h3");node.setAttribute("style",`font-size:${size};font-family:Times;font-weight:700`);node.textContent=text;document.body.append(node);
      const range=document.createRange();range.selectNodeContents(node);ranges.push({text,size,attribute:node.getAttribute("style"),width:range.getBoundingClientRect().width});
    }
  }
  const canvas=document.createElement("canvas"),context=canvas.getContext("2d"),canvases=[];
  for(const font of ["700 18.72px Times","bold 18.72px Times","400 18.72px Times","700 18.71875px Times","700 18.72px 'Times New Roman'","400 13.3333px Arial"]){
    context.font=font;canvases.push({font,accepted:context.font,Old:context.measureText("Old").width,result:context.measureText("result").width,full:context.measureText("Old result").width});
  }
  return {ranges,canvases};
})()
