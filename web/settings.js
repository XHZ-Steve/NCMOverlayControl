'use strict';
const $=id=>document.getElementById(id);
let token,busy=false,loaded;
function status(message,failed=false){$('settings-status').textContent=message;$('settings-status').dataset.error=String(failed);}
function lock(value){busy=value;document.querySelectorAll('input,button').forEach(el=>el.disabled=value||!loaded);}
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
 return {webPort,debugPort,fallback:$('fallback').checked,ncmPath:$('ncm-path').value.trim(),autoDebug:$('auto-debug').checked};
}
async function save(settings){
 const result=await request('/api/settings',settings);loaded=settings;status('已保存');
 if(new URL(result.url).origin!==location.origin)location.replace(result.url);
 return result;
}
$('preferences').onsubmit=async event=>{event.preventDefault();if(busy)return;const value=readSettings();if(!value)return;if(value.autoDebug&&!loaded.autoDebug&&!confirm('启动时可能重启网易云并中断播放，是否开启？'))return;lock(true);try{await save(value);}catch(error){status(error.message,true);}finally{lock(false);}};
$('connect').onclick=async()=>{if(busy)return;const value=readSettings();if(!value)return;if(value.webPort!==loaded.webPort){status('请先保存网页端口',true);return;}if(!confirm('将保存设置，并启动或重启网易云。当前播放可能中断。'))return;lock(true);try{await save(value);await request('/api/connect');status('已启动网易云');}catch(error){status(error.message,true);}finally{lock(false);}};
$('detect').onclick=async()=>{if(busy)return;lock(true);try{$('ncm-path').value=(await request('/api/detect')).path;status('已找到网易云');}catch(error){status(error.message,true);}finally{lock(false);}};
$('copy').onclick=async()=>{try{await navigator.clipboard.writeText(location.origin+'/');status('地址已复制');}catch{status('复制失败，请手动复制地址',true);}};
(async()=>{lock(true);try{const session=await(await fetch('/session',{cache:'no-store'})).json();token=session.token;const response=await fetch('/api/settings',{cache:'no-store'});if(!response.ok)throw Error('读取设置失败');loaded=await response.json();$('web-port').value=loaded.webPort;$('debug-port').value=loaded.debugPort;$('fallback').checked=loaded.fallback;$('ncm-path').value=loaded.ncmPath;$('auto-debug').checked=loaded.autoDebug;$('control-url').textContent=location.origin+'/';}catch(error){status(error.message,true);}finally{lock(false);}})();
