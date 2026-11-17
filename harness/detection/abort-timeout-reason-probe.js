(async()=>{
 let frame=document.createElement('iframe');document.body.append(frame);let child=frame.contentWindow;
 const out={};
 async function timed(sourceRealm,dependentRealm){
  let source=sourceRealm.AbortSignal.timeout(10),dependent=dependentRealm.AbortSignal.any([source]);
  const before=[source.aborted,dependent.aborted];
  await new Promise(resolve=>source.addEventListener('abort',resolve,{once:true}));
  return {before,same:dependent.reason===source.reason,stable:dependent.reason===dependent.reason,sourceBrand:source.reason instanceof sourceRealm.DOMException,dependentSourceBrand:dependent.reason instanceof sourceRealm.DOMException,dependentOwnBrand:dependent.reason instanceof dependentRealm.DOMException,name:dependent.reason.name,message:dependent.reason.message};
 }
 out.same=await timed(window,window);
 out.childSource=await timed(child,window);
 out.rootSource=await timed(window,child);
 let rootController=new AbortController(),childDependent=child.AbortSignal.any([rootController.signal]);
 let childController=new child.AbortController(),rootDependent=AbortSignal.any([childController.signal]);
 frame.remove();
 let rootReason={source:'root'},childReason=new child.DOMException('child authored','AbortError');
 rootController.abort(rootReason);childController.abort(childReason);
 out.retired={rootSource:childDependent.reason===rootReason,childSource:rootDependent.reason===childReason,childBrand:rootDependent.reason instanceof child.DOMException};
 let lifetimeFrame=document.createElement('iframe');document.body.append(lifetimeFrame);let lifetimeChild=lifetimeFrame.contentWindow;let lifetimeSource=lifetimeChild.AbortSignal.timeout(10);let lifetimeDependent=AbortSignal.any([lifetimeSource]);await new Promise(resolve=>lifetimeSource.addEventListener('abort',resolve,{once:true}));let lifetimeReason=lifetimeDependent.reason;out.dependentOnlyTimeoutReason={same:lifetimeReason===lifetimeSource.reason,sourceBrand:lifetimeReason instanceof lifetimeChild.DOMException,rootBrand:lifetimeReason instanceof DOMException};globalThis.reasonLifetime={dependent:lifetimeDependent,weak:new WeakRef(lifetimeReason),source:new WeakRef(lifetimeSource),child:new WeakRef(lifetimeChild)};lifetimeFrame.remove();
 (()=>{const unusedFrame=document.createElement('iframe');document.body.append(unusedFrame);const unusedChild=unusedFrame.contentWindow;const unusedController=new unusedChild.AbortController();const winner=new AbortController();let dependent=AbortSignal.any([winner.signal,unusedController.signal]);globalThis.unusedSourceRefs=[new WeakRef(unusedChild),new WeakRef(unusedController.signal)];globalThis.liveReasonDependent=dependent;unusedFrame.remove();winner.abort('root wins');if(dependent.reason!=='root wins')throw new Error('Wrong winning source');})();
 let timedFrame=document.createElement('iframe');document.body.append(timedFrame);let retiredTimed=timedFrame.contentWindow.AbortSignal.timeout(20);let retiredTimedDependent=AbortSignal.any([retiredTimed]);retiredTimedDependent.addEventListener('abort',()=>{});timedFrame.remove();await new Promise(resolve=>setTimeout(resolve,50));out.retiredTimeout={source:retiredTimed.aborted,dependent:retiredTimedDependent.aborted};
 globalThis.pendingDependencyRefs=[];globalThis.pendingDependents=[];
 for(const listen of [false,true]){let pendingFrame=document.createElement('iframe');document.body.append(pendingFrame);let pendingChild=pendingFrame.contentWindow;let source=new pendingChild.AbortController().signal;let dependent=AbortSignal.any([source]);if(listen)dependent.addEventListener('abort',()=>{});pendingDependencyRefs.push([new WeakRef(pendingChild),new WeakRef(source)]);pendingDependents.push(dependent);pendingFrame.remove();pendingFrame=null;pendingChild=null;source=null;dependent=null;}
 frame=null;child=null;rootController=null;childDependent=null;childController=null;rootDependent=null;rootReason=null;childReason=null;lifetimeFrame=null;lifetimeChild=null;lifetimeSource=null;lifetimeDependent=null;lifetimeReason=null;timedFrame=null;retiredTimed=null;retiredTimedDependent=null;
 return JSON.stringify(out);
})()
