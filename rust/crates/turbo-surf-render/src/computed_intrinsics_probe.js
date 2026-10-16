(() => {
  const target=document.getElementById("target"), style=document.querySelector("style");
  const read=getComputedStyle, result={values:[],matchesReads:0,ownerReads:0};
  const measure=()=>{const value=read(target);result.values.push([value.width,value.color]);};
  measure();
  Object.defineProperty(target,"matches",{configurable:true,get(){result.matchesReads++;throw Error("public matches getter");}});
  measure();delete target.matches;
  const saved=Object.getOwnPropertyDescriptor(Element.prototype,"matches");
  Object.defineProperty(Element.prototype,"matches",{...saved,value:function(){result.matchesReads++;return false;}});
  try {
    measure();
    Object.defineProperty(target,"ownerDocument",{configurable:true,get(){result.ownerReads++;throw Error("public owner getter");}});
    measure();style.sheet.cssRules[0].style.setProperty("width","23px");measure();
  } finally {Object.defineProperty(Element.prototype,"matches",saved);delete target.ownerDocument;}
  return result;
})();
