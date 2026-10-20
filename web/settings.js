'use strict';
const $=id=>document.getElementById(id);
let token,busy=false,loaded,connected=false,source;
function status(message,failed=false){$('settings-status').textContent=message;$('settings-status').dataset.error=String(failed);}
function updateConnect(){const ready=connected&&Number($('debug-port').value)===loaded?.debugPort;$('connect').disabled=busy||!loaded||ready;$('connect').title=ready?'已连接调试模式的网易云':'';}
function lock(value){busy=value;document.querySelectorAll('input,button').forEach(el=>el.disabled=value||!loaded);updateConnect();}
function subscribe(){
 if(source||!token||document.hidden)return;
 const stream=new EventSource('/events?token='+encodeURIComponent(token));source=stream;
 stream.onmessage=event=>{if(source!==stream)return;try{connected=JSON.parse(event.data).connected===true;updateConnect();}catch{}};
 stream.onerror=()=>{if(source===stream){connected=false;updateConnect();}};
}
function unsubscribe(){source?.close();source=null;connected=false;updateConnect();}
document.addEventListener('visibilitychange',()=>document.hidden?unsubscribe():subscribe());
window.addEventListener('pagehide',unsubscribe);window.addEventListener('pageshow',subscribe);
$('debug-port').addEventListener('input',updateConnect);
async function request(path,body){
 const controller=new AbortController(),timer=setTimeout(()=>controller.abort(),30000);
 try{const response=await fetch(path,{method:'POST',headers:{'Content-Type':'application/json','X-Overlay-Token':token},body:JSON.stringify(body||{}),signal:controller.signal});const result=await response.json();if(!response.ok)throw Error(result.error||'操作失败');return result;}finally{clearTimeout(timer);}
}
function readSettings(){
 let invalid;
 const parse=(id,errorId)=>{const el=$(id),n=Number(el.value);const error=!/^\d+$/.test(el.value)||!Number.isInteger(n)||n<1||n>65535?'请输入 1–65535 的整数':'';el.setAttribute('aria-invalid',String(!!error));$(errorId).textContent=error;if(error&&!invalid)invalid=el;return n;};
 const webPort=parse('web-port','web-error'),debugPort=parse('debug-port','debug-error');
 if(!invalid&&webPort===debugPort){$('debug-error').textContent='两个端口不能相同';$('debug-port').setAttribute('aria-invalid','true');invalid=$('debug-port');}
 if(invalid){invalid.focus();return null;}
 return {webPort,debugPort,fallback:$('fallback').checked,ncmPath:$('ncm-path').value.trim(),autoDebug:$('auto-debug').checked,showSettings:$('show-settings').checked};
}
async function save(settings){
 const result=await request('/api/settings',settings);if(settings.debugPort!==loaded.debugPort)connected=false;loaded=settings;status('已保存');
 if(new URL(result.url).origin!==location.origin)location.replace(result.url);
 return result;
}
$('preferences').onsubmit=async event=>{event.preventDefault();if(busy)return;const value=readSettings();if(!value)return;if(value.autoDebug&&!loaded.autoDebug&&!confirm('控件运行期间，普通模式启动的网易云将自动重启为调试模式，可能中断播放。是否开启？'))return;lock(true);try{await save(value);}catch(error){status(error.message,true);}finally{lock(false);}};
$('connect').onclick=async()=>{if($('connect').disabled)return;const value=readSettings();if(!value)return;if(value.webPort!==loaded.webPort){status('请先保存网页端口',true);return;}if(!confirm('将保存设置，并以调试模式启动或重启网易云。当前播放可能中断。'))return;lock(true);try{await save(value);await request('/api/connect');status('已以调试模式启动网易云');}catch(error){status(error.message,true);}finally{lock(false);}};
$('detect').onclick=async()=>{if(busy)return;lock(true);try{$('ncm-path').value=(await request('/api/detect')).path;status('已找到网易云');}catch(error){status(error.message,true);}finally{lock(false);}};
$('copy').onclick=async()=>{try{await navigator.clipboard.writeText(location.origin+'/');status('地址已复制');}catch{status('复制失败，请手动复制地址',true);}};
(async()=>{lock(true);try{const session=await(await fetch('/session',{cache:'no-store'})).json();token=session.token;const response=await fetch('/api/settings',{cache:'no-store'});if(!response.ok)throw Error('读取设置失败');loaded=await response.json();$('web-port').value=loaded.webPort;$('debug-port').value=loaded.debugPort;$('fallback').checked=loaded.fallback;$('ncm-path').value=loaded.ncmPath;$('auto-debug').checked=loaded.autoDebug;$('show-settings').checked=loaded.showSettings!==false;$('control-url').textContent=location.origin+'/';subscribe();}catch(error){status(error.message,true);}finally{lock(false);}})();
