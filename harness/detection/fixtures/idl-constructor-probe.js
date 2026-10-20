(() => {
  const errorName=operation=>{try{operation();return null;}catch(error){return error.name;}};
  const pairs=[[Node,EventTarget],[Element,Node],[HTMLElement,Element],[HTMLDivElement,HTMLElement],[Document,Node],[HTMLDocument,Document],[DocumentFragment,Node],[Text,CharacterData],[HTMLAudioElement,HTMLMediaElement],[AbortSignal,EventTarget]];
  const constructors=pairs.map(([Constructor,Parent])=>{
    let created=null,constructionError=null;
    try{const node=new Constructor();created={prototype:Object.getPrototypeOf(node)===Constructor.prototype,nodeType:node.nodeType,nodeName:node.nodeName,ownerDocumentIsCurrent:node.ownerDocument===document,ownerDocumentIsNull:node.ownerDocument===null,parentNodeIsNull:node.parentNode===null,childCount:node.childNodes.length,data:node.data??null,defaultViewIsNull:node.nodeType===9?node.defaultView===null:null,contentType:node.contentType??null};}catch(error){constructionError=error.name;}
    return {name:Constructor.name,parent:Parent.name,parentIdentity:Object.getPrototypeOf(Constructor)===Parent,prototypeParentIdentity:Object.getPrototypeOf(Constructor.prototype)===Parent.prototype,prototypeWritable:Object.getOwnPropertyDescriptor(Constructor,'prototype').writable,globalEnumerable:Object.getOwnPropertyDescriptor(globalThis,Constructor.name).enumerable,callError:errorName(()=>Constructor()),constructionError,created};
  });
  const host=document.createElement('div');document.body.append(host);const root=host.attachShadow({mode:'open'});root.innerHTML='<span id="probe-shadow">text</span>';const span=root.firstChild;
  const cloneError=errorName(()=>root.cloneNode(true)),duplicateError=errorName(()=>host.attachShadow({mode:'open'}));
  let returned=null,appendError=null;try{returned=document.body.appendChild(root);}catch(error){appendError=error.name;}
  const shadow={cloneError,duplicateError,appendError,returnedRoot:returned===root,emptyRoot:root.childNodes.length===0,movedChild:span.parentNode===document.body,rootHost:root.host===host,hostRoot:host.shadowRoot===root,rootParentNull:root.parentNode===null,rootDocument:root.ownerDocument===document,childDocumentRoot:span.getRootNode()===document};
  span.remove();host.remove();
  return JSON.stringify({constructors,shadow,eventTarget:{constructorParentIsFunctionPrototype:Object.getPrototypeOf(EventTarget)===Function.prototype,nodeConstructorParent:Object.getPrototypeOf(Node)===EventTarget,nodePrototypeParent:Object.getPrototypeOf(Node.prototype)===EventTarget.prototype,abortConstructorParent:Object.getPrototypeOf(AbortSignal)===EventTarget,abortPrototypeParent:Object.getPrototypeOf(AbortSignal.prototype)===EventTarget.prototype}});
})()
