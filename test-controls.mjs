import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const html=readFileSync(new URL('./web/index.html',import.meta.url),'utf8');
assert.doesNotMatch(html,/transport-hint|dislike-help|选择一项，按网易云原生流程处理/);
const nodes=new Map(),requests=[],storage=new Map();
function node(id){if(!nodes.has(id))nodes.set(id,{id,dataset:{},value:'',disabled:false,textContent:'',listeners:{},setAttribute(){},removeAttribute(){},querySelector:()=>({}),addEventListener(event,fn){this.listeners[event]=fn;}});return nodes.get(id);}
const commands=['previous','next','toggle','like','dislike','mute'];
for(const cmd of commands)node(cmd).dataset.command=cmd;
const context={document:{hidden:true,body:{dataset:{}},getElementById:node,querySelectorAll:selector=>selector.includes('#mode')?[...commands.map(node),node('mode'),node('volume'),node('volume-number')]:commands.map(node),addEventListener(){}},window:{addEventListener(){}},localStorage:{getItem:key=>storage.get(key),setItem:(key,value)=>storage.set(key,value)},AbortController,setTimeout:()=>1,clearTimeout(){},fetch:async(_,options)=>{requests.push(JSON.parse(options.body));return{ok:true,json:async()=>({mediaSent:true,transport:'windows'})};}};
vm.createContext(context);vm.runInContext(readFileSync(new URL('./web/app.js',import.meta.url),'utf8'),context);
assert.equal(node('transport').value,'windows');
vm.runInContext("token='test';serviceAvailable=true;render({connected:false,error:'NCM offline'});",context);
assert.equal(node('toggle').disabled,false);assert.equal(node('like').disabled,true);
await vm.runInContext("send('toggle',undefined,$('toggle'))",context);
assert.deepEqual(requests[0],{command:'toggle',transport:'windows'});
assert.equal(vm.runInContext('state.connected',context),false,'media ACK must not overwrite real NCM state');
node('transport').value='ncm';node('transport').listeners.change();
assert.equal(storage.get('overlayMusic.transport'),'ncm');assert.equal(node('toggle').disabled,true);
vm.runInContext("render({connected:true,title:'Test',volume:100,playing:true,canAI:true,canLike:true});",context);
await vm.runInContext("send('next',undefined,$('next'))",context);
assert.deepEqual(requests[1],{command:'next',transport:'ncm'});
await vm.runInContext("send('like',undefined,$('like'))",context);
assert.deepEqual(requests[2],{command:'like'},'other controls keep the original NCM route');
for(const mode of ['playOrder','playCycle','playOneCycle','playRandom','playAi','playFm']){
 vm.runInContext(`render({connected:true,title:'Test',mode:${JSON.stringify(mode)},canLike:true,canDislike:true});`,context);
 assert.equal(node('dislike').disabled,false,'Dislike must be usable in '+mode);
}
await vm.runInContext("send('mode','playFm',$('mode'))",context);assert.deepEqual(requests.at(-1),{command:'mode',value:'playFm'});
vm.runInContext("render({connected:true,title:'Test',mode:'playFm',modePending:'playAi',canDislike:true});",context);
assert.equal(node('mode').disabled,true);assert.equal(node('dislike').disabled,false);
vm.runInContext("render({connected:true,title:'Test',mode:'playFm',operationError:'Native mode failed',canDislike:true});",context);
assert.equal(node('mode').disabled,false);assert.equal(node('error').textContent,'Native mode failed');
console.log('Control transport tests passed');
