'use strict';
const $ = id => document.getElementById(id);
let token, source, state = {connected:false}, busy = false, paused = false, stopped = false, starting = false, reconnect, pendingVolume, volumeRunning = false;
let transport='windows', serviceAvailable=false;
let ncmRunning=false,checkingNcm=false,lastNcmCheck=0,connectingNcm=false;
async function checkNcm(){
 if(checkingNcm||state.connected||stopped||paused||document.hidden||!serviceAvailable||Date.now()-lastNcmCheck<3000)return;
 checkingNcm=true;lastNcmCheck=Date.now();
 try{const r=await fetch('/api/ncm-status',{cache:'no-store'});if(r.ok)ncmRunning=(await r.json()).running===true;}catch{}finally{checkingNcm=false;updateQuickConnect();}
}
function updateQuickConnect(){const button=$('quick-connect');button.hidden=state.connected||!ncmRunning||!serviceAvailable||stopped;button.disabled=connectingNcm;}
try {if(localStorage.getItem('ncmOverlayControl.transport')==='ncm') transport='ncm';} catch {}
$('transport').value=transport;
const mediaCommands=new Set(['previous','next','toggle']);
function error(text) { $('error').textContent = text || ''; }
function render(s) {
 const wasModePending=state.modePending;
 state = s; document.body.dataset.connected = !!s.connected;
 $('status').textContent = stopped ? '控件已退出' : paused ? '更新已暂停' : s.connected ? '已连接网易云' : '连接已断开';
 if (s.connected) {
   $('title').textContent = s.title; $('title').title = s.title; $('artist').textContent = s.artist || '未知歌手';
   const label = s.playing ? '暂停' : '播放'; $('toggle').title = label; $('toggle').setAttribute('aria-label',label);
   $('play-icon').setAttribute('d',s.playing ? 'M8 5v14M16 5v14' : 'M8 5l11 7-11 7z');
   $('like').setAttribute('aria-pressed',!!s.liked); $('like').setAttribute('aria-label',s.liked?'取消喜欢':'喜欢');
   $('mute').setAttribute('aria-pressed',s.volume===0); $('mute').setAttribute('aria-label',s.volume===0?'恢复音量':'静音');
   if(document.activeElement!==$('volume') && document.activeElement!==$('volume-number') && !volumeRunning){$('volume').value=s.volume;$('volume-number').value=s.volume;}
   $('mode').value=s.mode;
 }
 for(const el of document.querySelectorAll('[data-command],#mode,#volume,#volume-number')) el.disabled=!s.connected || busy;
 for(const cmd of mediaCommands) if(transport==='windows') $(cmd).disabled=!serviceAvailable || busy || stopped;
 $('transport').disabled=busy || stopped;
 $('like').disabled ||= !s.canLike; $('dislike').disabled ||= !s.canDislike;
 $('mode').disabled ||= !!s.modePending;
 if(s.modePending)$('control-feedback').textContent='正在切换播放模式，等待网易云确认…';
 else if(wasModePending&&!s.operationError)$('control-feedback').textContent='播放模式已更新';
 if(s.operationError)error(s.operationError);
 if(!s.connected) error(s.error || '本地服务断开，请确认程序仍在运行');
 updateQuickConnect();if(!s.connected)checkNcm();
}
function stop() {source?.close();source=null;clearTimeout(reconnect);}
async function start() {
 if(document.hidden || paused || source || starting) return;
 starting=true;
 try {
   const r=await fetch('/session',{cache:'no-store'});if(!r.ok)throw Error('本地服务不可用');token=(await r.json()).token;serviceAvailable=true;render(state);
   if(document.hidden || paused) return;
   const connection=new EventSource('/events?token='+encodeURIComponent(token));source=connection;
   connection.onmessage=e=>{if(source!==connection)return;try{const s=JSON.parse(e.data);if(s.connected&&!state.connected)error('');render(s);}catch{error('无效状态响应');}};
   connection.onerror=()=>{if(source!==connection)return;serviceAvailable=false;stop();render({...state,connected:false,error:'连接断开，3 秒后重试'});reconnect=setTimeout(start,3000);};
 } catch(e){serviceAvailable=false;if(!paused){render({...state,connected:false,error:e.message});reconnect=setTimeout(start,3000);}}finally{starting=false;}
}
async function send(command,value,element) {
 if(busy) return; if(paused){paused=false;$('power').textContent='暂停更新';$('power').setAttribute('aria-pressed','false');start();}
 if(command==='shutdown'){paused=true;stop();}
 busy=true;render(state);error('');if(element)element.dataset.state='loading';
 const abort=new AbortController(), timeout=setTimeout(()=>abort.abort(),12000);
 try {
   const r=await fetch('/command',{method:'POST',headers:{'Content-Type':'application/json','X-Overlay-Token':token},body:JSON.stringify({command,value,...(mediaCommands.has(command)?{transport}:{})}),signal:abort.signal});
   const s=await r.json();if(!r.ok)throw Error(s.error || '操作失败');if(!s.mediaSent)render(s);if(s.controlMessage)$('control-feedback').textContent=s.controlMessage;if(element)element.dataset.state=s.controlResult==='unconfirmed'?'error':'success';
   if(command==='shutdown'){paused=true;stopped=true;stop();$('power').disabled=true;$('exit').disabled=true;}
   if(s.dislikeChoices){$('dislike-choices').replaceChildren();s.dislikeChoices.forEach((text,index)=>{const b=document.createElement('button');b.type='button';b.textContent=text;b.title=text;b.onclick=()=>{$('dislike-dialog').close();send('dislikeReason',index,b);};$('dislike-choices').append(b);});$('dislike-dialog').showModal();$('dislike-choices').scrollTop=0;}
 } catch(e){error(e.message || '操作失败，请重新操作');if(element)element.dataset.state='error';}
 finally {clearTimeout(timeout);busy=false;render(state);if(element)setTimeout(()=>delete element.dataset.state,1500);}
}
for(const el of document.querySelectorAll('[data-command]'))el.addEventListener('click',()=>send(el.dataset.command,undefined,el));
$('transport').addEventListener('change',()=>{transport=$('transport').value;try{localStorage.setItem('ncmOverlayControl.transport',transport);}catch{}error('');render(state);});
$('mode').addEventListener('change',()=>send('mode',$('mode').value,$('mode')));
async function drainVolume() {
 if(volumeRunning)return;volumeRunning=true;
 try{while(pendingVolume!==undefined){if(busy){await new Promise(r=>setTimeout(r,100));continue;}const v=pendingVolume;pendingVolume=undefined;await send('volume',v,$('volume-number'));}}finally{volumeRunning=false;render(state);}
}
$('volume').addEventListener('input',()=>{$('volume-number').value=$('volume').value;});
// Commit on release, avoiding CDP/native writes for every mouse pixel.
$('volume').addEventListener('change',()=>{pendingVolume=Number($('volume').value);drainVolume();});
$('volume-number').addEventListener('change',()=>{const n=Number($('volume-number').value);if($('volume-number').value===''||!Number.isInteger(n)||n<0||n>100){$('volume-number').setAttribute('aria-invalid','true');error('请输入 0–100 的整数音量');return;}$('volume-number').removeAttribute('aria-invalid');$('volume').value=n;pendingVolume=n;drainVolume();});
$('volume-number').addEventListener('keydown',e=>{if(e.key==='Enter'){$('volume-number').dispatchEvent(new Event('change'));$('volume-number').blur();}});
$('power').addEventListener('click',()=>{paused=!paused;$('power').textContent=paused?'恢复更新':'暂停更新';$('power').setAttribute('aria-pressed',paused);if(paused){stop();error('状态更新已暂停；点击播放等操作仍可控制。');}else{error('');start();}render(state);});
document.addEventListener('visibilitychange',()=>{if(document.hidden)stop();else start();});
window.addEventListener('pagehide',stop);window.addEventListener('pageshow',start);start();
$('dislike-cancel').onclick=()=>{$('dislike-dialog').close();send('dislikeCancel');};
$('dislike-dialog').addEventListener('cancel',()=>send('dislikeCancel'));
$('exit').onclick=()=>send('shutdown',undefined,$('exit'));
$('quick-connect').onclick=async()=>{
 if(connectingNcm||!confirm('将以调试模式重启网易云，当前播放可能中断。是否继续？'))return;
 connectingNcm=true;updateQuickConnect();error('正在以调试模式启动网易云…');
 const controller=new AbortController(),timer=setTimeout(()=>controller.abort(),30000);
 try{const r=await fetch('/api/connect',{method:'POST',headers:{'Content-Type':'application/json','X-Overlay-Token':token},body:'{}',signal:controller.signal});const result=await r.json();if(!r.ok)throw Error(result.error||'启动失败');error('已启动，正在等待连接…');}catch(e){error(e.message);}finally{clearTimeout(timer);connectingNcm=false;updateQuickConnect();}
};
setInterval(checkNcm,5000);
