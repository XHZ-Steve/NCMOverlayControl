import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const code=readFileSync(new URL('./web/adapter.js',import.meta.url),'utf8');
function fixture(){
  const p={playingState:2,resourceTrackId:'song-1',resourceArtists:[]};
  let dispatches=[],clicks=0,hook;
  const store={getState:()=>({playing:p}),subscribe:()=>()=>{},dispatch:action=>{dispatches.push(action);p.playingState=action.type==='playing/pause'?1:2;}};
  const button={getClientRects:()=>[1],closest:()=>null,querySelector:()=>null,getAttribute:()=>null,click:()=>{clicks++;p.resourceTrackId='song-2';}};
  const root={_reactRootContainer:{_internalRoot:{current:{child:{child:{memoizedProps:{store}}}}}}};
  const context={window:{__ncmOverlayControlNotify(){}},document:{getElementById:()=>root,querySelectorAll:()=>[button]},MutationObserver:class{observe(){}disconnect(){}},clearTimeout(){},setTimeout(fn,ms){if(ms<10000)queueMicrotask(()=>{if(ms===150)hook?.();fn();});return 1;}};
  vm.createContext(context);vm.runInContext(code,context);
  return{p,bridge:context.window.__ncmOverlayControlBridge,dispatches,get clicks(){return clicks;},set hook(fn){hook=fn;}};
}
{
  const f=fixture(),before=f.bridge.snapshot();
  const result=await f.bridge.finishMedia('toggle',before,null);
  assert.equal(result.controlResult,'ncm-fallback');assert.equal(f.p.playingState,1);
  assert.deepEqual(f.dispatches.map(a=>a.type),['playing/pause']);
}
{
  const f=fixture();f.p.playingState=1; // Like a physical/headset pause before the request.
  const result=await f.bridge.finishMedia('toggle',f.bridge.snapshot(),null);
  assert.equal(result.controlResult,'ncm-fallback');assert.equal(f.p.playingState,2);
  assert.deepEqual(f.dispatches.map(a=>a.type),['playing/resume']);
}
{
  const f=fixture(),before=f.bridge.snapshot();let ticks=0;
  f.hook=()=>{if(++ticks===5)f.p.playingState=1;};
  const result=await f.bridge.finishMedia('toggle',before,null);
  assert.equal(result.controlResult,'windows');assert.equal(f.dispatches.length,0);
}
{
  const f=fixture(),before=f.bridge.snapshot();f.p.playingState=1;
  assert.equal((await f.bridge.finishMedia('toggle',before,'SendInput failed')).controlResult,'windows');
  assert.equal(f.dispatches.length,0);
}
for(const cmd of ['next','previous']){
  const f=fixture(),before=f.bridge.snapshot();f.hook=()=>{f.p.resourceTrackId='external-song';};
  assert.equal((await f.bridge.finishMedia(cmd,before,null)).controlResult,'windows');assert.equal(f.clicks,0);
  const ignored=fixture();
  assert.equal((await ignored.bridge.finishMedia(cmd,ignored.bridge.snapshot(),null)).controlResult,'ncm-fallback');assert.equal(ignored.clicks,1);
}
{
  const f=fixture(),before=f.bridge.snapshot();f.p.resourceTrackId='external-song';
  await f.bridge.finishMedia('toggle',before,'failed');assert.equal(f.dispatches.length,0);
  await assert.rejects(f.bridge.finishMedia('like',before,null));
}
console.log('Fallback tests passed: ignored, physical-pause state, delayed, already responded, external track change, skip exactly once');
