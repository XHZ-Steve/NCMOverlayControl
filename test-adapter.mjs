import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const code=readFileSync(new URL('./web/adapter.js',import.meta.url),'utf8');
const p={playingVolume:.8,playingMode:'playOrder',resourceTrackId:'1',resourceName:'Test',resourceArtists:[{name:'Artist'}],playingState:1,curTrack:{id:'1'},curPlaying:{scene:'playlist'}};
let subscriber,disposed=0,clicks=0,modal=null,reasonClicked=0;
const actions=[],fm={currentScene:'fm'},nativeItem={textContent:'屏蔽歌曲：Test',isConnected:true,classList:{contains:()=>false},getAttribute:()=>null,click:()=>{reasonClicked++;modal=null;}};
const button={getClientRects:()=>[1],closest:()=>null,querySelector:()=>({title:'喜欢'}),getAttribute:()=>null,click:()=>{clicks++;p.playingState=p.playingState===2?1:2;subscriber();}};
const store={getState:()=>({playing:p,fmPlaying:fm}),subscribe:fn=>{subscriber=fn;return()=>disposed++;},dispatch:({type,payload})=>{actions.push({type,payload});if(type==='playing/setVolume')p.playingVolume=payload.volume;if(type==='playing/switchPlayingMode')p.playingMode=payload.playingMode;if(type==='playing/aiPlay'){p.playingMode='playAi';p.playingState=2;}if(type==='fmPlaying/fmPlay'){p.playingMode='playFm';p.playingState=2;}if(type==='playing/resume')p.playingState=2;if(type==='playing/pause')p.playingState=1;if(type==='playing/openDislikeModal'||type==='fmPlaying/openDislikeModal')modal={isConnected:true,contains:el=>el===nativeItem,querySelectorAll:()=>[nativeItem],querySelector:()=>({click:()=>{modal=null;}})};subscriber();}};
const root={_reactRootContainer:{_internalRoot:{current:{child:{child:{memoizedProps:{store}}}}}}};
const context={window:{__overlayMusicNotify:()=>{}},document:{getElementById:()=>root,querySelectorAll:()=>[button],querySelector:selector=>selector.includes('tid_blacklist')?modal?{closest:()=>modal}:null:modal},MutationObserver:class{observe(){}disconnect(){}},setTimeout,clearTimeout};vm.createContext(context);
const initial=vm.runInContext(code,context);assert.equal(initial.volume,80);assert.equal(initial.title,'Test');
const bridge=context.window.__overlayMusicBridge;
assert.equal((await bridge.command('volume',37)).volume,37);
assert.equal((await bridge.command('mode','playRandom')).mode,'playRandom');
await bridge.command('toggle');assert.equal(bridge.snapshot().playing,true);await bridge.command('next');assert.equal(clicks,1);
await assert.rejects(bridge.command('volume',101));await assert.rejects(bridge.command('mode','anything'));await assert.rejects(bridge.command('evaluate'));
await bridge.command('mode','playAi');assert.equal(actions.at(-1).type,'playing/aiPlay');assert.equal(p.playingMode,'playAi');
await bridge.command('mode','playFm');assert.equal(actions.at(-1).type,'fmPlaying/fmPlay');assert.equal(p.playingMode,'playFm');
const beforeRepeat=actions.length;await bridge.command('mode','playFm');assert.equal(actions.length,beforeRepeat,'Selecting current FM must not toggle/pause');
p.playingState=1;await bridge.command('mode','playFm');assert.equal(actions.at(-1).type,'playing/resume');
for(const mode of ['playOrder','playCycle','playOneCycle','playRandom','playAi','playFm']){
 p.playingMode=mode;assert.equal(bridge.snapshot().canDislike,true);
 assert.equal((await bridge.command('dislike')).dislikeChoices[0],'屏蔽歌曲：Test');
 assert.equal(actions.at(-1).type,'playing/openDislikeModal');assert.equal(actions.at(-1).payload.resource,p.curTrack);
 await bridge.command('dislikeCancel');assert.equal(modal,null);assert.equal(reasonClicked,0);
}
p.playingMode='playFm';assert.equal((await bridge.command('dislike')).dislikeChoices[0],'屏蔽歌曲：Test');await bridge.command('dislikeReason',0);assert.equal(reasonClicked,1);
await bridge.command('dislike');p.resourceTrackId='2';await assert.rejects(bridge.command('dislikeReason',0));assert.equal(reasonClicked,1);assert.equal(modal,null);
bridge.dispose();assert.equal(disposed,1);assert.equal(context.window.__overlayMusicBridge,undefined);console.log('Adapter tests passed');

// Native async operations: don't fake mode, re-dispatch while pending, or
// turn a failed native entry into a successful selection.
let resolveMode,rejectMode;
const normalDispatch=store.dispatch;
store.dispatch=action=>{actions.push(action);return new Promise((resolve,reject)=>{resolveMode=resolve;rejectMode=reject;});};
vm.runInContext(code,context);const asyncBridge=context.window.__overlayMusicBridge;
p.playingMode='playFm';
assert.equal((await asyncBridge.command('mode','playAi')).modePending,'playAi');
await assert.rejects(asyncBridge.command('mode','playRandom'),/正在等待/);
p.playingMode='playAi';resolveMode();await new Promise(r=>setTimeout(r,0));assert.equal(asyncBridge.snapshot().modePending,'');
await asyncBridge.command('mode','playFm');rejectMode(Error('native failure'));await new Promise(r=>setTimeout(r,0));
assert.match(asyncBridge.snapshot().operationError,/切换失败/);assert.equal(asyncBridge.snapshot().mode,'playAi');
store.dispatch=normalDispatch;
await asyncBridge.command('dislike');nativeItem.textContent='Changed option';await assert.rejects(asyncBridge.command('dislikeReason',0),/选项已变化/);assert.equal(reasonClicked,1);
nativeItem.textContent='屏蔽歌曲：Test';
await asyncBridge.command('dislike');modal.isConnected=false;await assert.rejects(asyncBridge.command('dislikeReason',0),/原因框已关闭/);assert.equal(reasonClicked,1);modal=null;
asyncBridge.dispose();console.log('Async mode and stale-feedback checks passed');
