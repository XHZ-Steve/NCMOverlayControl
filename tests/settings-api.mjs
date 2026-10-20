import {spawn,execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {createServer} from 'node:http';
import {setTimeout as delay} from 'node:timers/promises';
import assert from 'node:assert/strict';
const ps=code=>execFileSync('powershell.exe',['-NoProfile','-NonInteractive','-EncodedCommand',Buffer.from(code,'utf16le').toString('base64')],{encoding:'utf8',windowsHide:true}).trim().replace(/^\uFEFF/,'');
const original=ps("[Console]::OutputEncoding=[Text.Encoding]::UTF8;$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\\NCMOverlayControl');if($k){$v=$k.GetValue('Settings',$null);$k.Close();@{value=$v}|ConvertTo-Json -Compress}else{'{\"value\":null}'}");
const originalValue=JSON.parse(original).value;
const port=async()=>{const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;};
const web=await port(),debug=await port(),newPort=await port();
const executable=process.env.OM_TEST_EXE||fileURLToPath(new URL('../target/release/ncm-overlay-control.exe',import.meta.url));
const child=spawn(executable,['--headless','--port',String(web),'--debug-port',String(debug)],{windowsHide:true});let base=`http://127.0.0.1:${web}`,session;
const post=(path,value,headers={})=>fetch(base+path,{method:'POST',headers:{'Content-Type':'application/json','X-Overlay-Token':session.token,...headers},body:JSON.stringify(value)});
try{
 for(let i=0;i<100;i++){try{session=await(await fetch(base+'/session')).json();break;}catch{}await delay(50);}
 assert.ok(session,'Server did not start');
 assert.equal((await fetch(base+'/settings')).status,200);assert.equal((await fetch(base+'/licenses')).status,200);
 const initial=await(await fetch(base+'/api/settings')).json();assert.equal(initial.webPort,web);assert.equal(initial.debugPort,debug);
 assert.equal((await post('/api/settings',initial,{'X-Overlay-Token':'wrong'})).status,403);
 for(const change of [{webPort:0},{debugPort:65536},{debugPort:web},{fallback:'true'},{ncmPath:'cmd.exe'},{extra:true}])assert.equal((await post('/api/settings',{...initial,...change})).status,400);
 const conflict=createServer();await new Promise(r=>conflict.listen(0,'127.0.0.1',r));
 assert.equal((await post('/api/settings',{...initial,webPort:conflict.address().port})).status,400);await new Promise(r=>conflict.close(r));
 assert.equal((await fetch(base+'/session')).status,200,'Conflict must keep old listener');
 const next={...initial,webPort:newPort,fallback:false,ncmPath:'',autoDebug:false};let response=await post('/api/settings',next);assert.equal(response.status,200);assert.equal((await response.json()).url,`http://127.0.0.1:${newPort}/settings`);
 base=`http://127.0.0.1:${newPort}`;assert.deepEqual(await(await fetch(base+'/api/settings')).json(),next);assert.equal((await(await fetch(base+'/session')).json()).fallback,false);
 let changedDebug=await port();while(changedDebug===newPort)changedDebug=await port();
 assert.equal((await post('/api/settings',{...next,debugPort:changedDebug})).status,200);
 const stored=JSON.parse(ps("$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\\NCMOverlayControl');[Console]::Write($k.GetValue('Settings'));$k.Close()"));assert.equal(stored.webPort,newPort);
 assert.equal((await post('/api/connect',{})).status,400,'Empty path must not launch/stop NCM');
 await post('/command',{command:'shutdown'});for(let i=0;i<100&&child.exitCode===null;i++)await delay(50);assert.equal(child.exitCode,0);
 console.log('Settings API passed: registry persistence, live port switching, conflict rollback, validation/authentication, fallback updates, empty-path safety and clean shutdown.');
}finally{
 if(child.exitCode===null){child.kill();await delay(100);}
 if(originalValue!==null){const value=Buffer.from(originalValue,'utf8').toString('base64');ps(`$k=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\\NCMOverlayControl');$k.SetValue('Settings',[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('${value}')));$k.Close()`);}else{ps("$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\\NCMOverlayControl',$true);if($k){$k.DeleteValue('Settings',$false);$empty=$k.GetValueNames().Count -eq 0 -and $k.GetSubKeyNames().Count -eq 0;$k.Close();if($empty){[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKey('Software\\NCMOverlayControl',$false)}}");}
}
