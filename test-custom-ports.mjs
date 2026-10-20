import {createServer,request as httpRequest} from 'node:http';
import {createHash} from 'node:crypto';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {setTimeout as delay} from 'node:timers/promises';
import assert from 'node:assert/strict';

// Isolated mock debugger: no real NCM process or playback commands are used.
const sockets=new Set(),methods=[];let discoveryHost;
const mockState={connected:true,title:'Mock NCM on custom port',id:'test',volume:50,playing:false};
const debuggerServer=createServer((request,response)=>{
  if(request.url!=='/json/list'){response.writeHead(404);response.end();return;}
  discoveryHost=request.headers.host;
  const body=JSON.stringify([{type:'page',url:'orpheus://orpheus/pub/app.html',webSocketDebuggerUrl:`ws://localhost:${debugPort}/devtools/page/MOCK`}]);
  response.writeHead(200,{'Content-Type':'application/json','Content-Length':Buffer.byteLength(body)});response.end(body);
});
debuggerServer.on('upgrade',(request,socket,head)=>{
  sockets.add(socket);socket.on('close',()=>sockets.delete(socket));
  const accept=createHash('sha1').update(request.headers['sec-websocket-key']+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  let buffered=Buffer.from(head);
  const consume=data=>{
    buffered=Buffer.concat([buffered,data]);
    while(buffered.length>=2){
      let length=buffered[1]&127,offset=2;
      if(length===126){if(buffered.length<4)return;length=buffered.readUInt16BE(2);offset=4;}
      if(length===127){if(buffered.length<10)return;length=Number(buffered.readBigUInt64BE(2));offset=10;}
      const masked=Boolean(buffered[1]&128),start=offset+(masked?4:0);
      if(buffered.length<start+length)return;
      const opcode=buffered[0]&15,payload=Buffer.from(buffered.subarray(start,start+length));
      if(masked)for(let i=0;i<length;i++)payload[i]^=buffered[offset+i%4];
      buffered=buffered.subarray(start+length);
      if(opcode===8){socket.end();return;}if(opcode!==1)continue;
      const rpc=JSON.parse(payload.toString());methods.push(rpc.method);
      const result=rpc.method==='Runtime.evaluate'?{result:{type:'object',value:mockState}}:{};
      const body=Buffer.from(JSON.stringify({id:rpc.id,result}));
      const header=body.length<126?Buffer.from([129,body.length]):Buffer.from([129,126,body.length>>>8,body.length&255]);
      socket.write(Buffer.concat([header,body]));
    }
  };
  socket.on('data',consume);if(head.length)consume(Buffer.alloc(0));
});
await new Promise(resolve=>debuggerServer.listen(0,'127.0.0.1',resolve));
const debugPort=debuggerServer.address().port;
const reservation=createServer();await new Promise(resolve=>reservation.listen(0,'127.0.0.1',resolve));
const webPort=reservation.address().port;await new Promise(resolve=>reservation.close(resolve));
const exe=process.env.OM_TEST_EXE||fileURLToPath(new URL('./target/release/ncm-overlay-control.exe',import.meta.url));
const child=spawn(exe,['--headless','--port',String(webPort),'--debug-port',String(debugPort),'--disable-media-fallback'],{windowsHide:true});
let stderr='';child.stderr.on('data',chunk=>stderr+=chunk);child.on('error',error=>stderr+=error.message);
const base=`http://127.0.0.1:${webPort}`;let session,streamAbort,pump;
try{
  for(let i=0;i<100;i++){
    if(child.exitCode!==null)throw Error('Server exited: '+stderr);
    try{const response=await fetch(base+'/session',{signal:AbortSignal.timeout(500)});if(response.ok){session=await response.json();break;}}catch{}
    await delay(50);
  }
  assert.ok(session,'Custom web port did not start: '+stderr);assert.equal(session.webPort,webPort);assert.equal(session.debugPort,debugPort);assert.equal(session.fallback,false);
  for(const headers of [{Host:'127.0.0.1:17890'},{Origin:'http://127.0.0.1:17890'},{Origin:'http://evil.invalid'},{Host:`localhost:${webPort}.evil`}]){
    // fetch rewrites Host; raw HTTP is required to exercise the real header.
    const status=await new Promise((resolve,reject)=>{const request=httpRequest(base+'/session',{headers},response=>{response.resume();resolve(response.statusCode);});request.on('error',reject);request.end();});
    assert.equal(status,403,JSON.stringify(headers));
  }
  assert.equal((await fetch(base+'/session',{headers:{Origin:`http://localhost:${webPort}`}})).status,200);
  assert.equal((await fetch(base+'/app.js')).status,200);
  assert.equal((await fetch(base+'/command',{method:'POST',headers:{'Content-Type':'application/json'},body:'{"command":"next"}'})).status,403);
  streamAbort=new AbortController();const events=await fetch(base+'/events?token='+session.token,{signal:streamAbort.signal});
  let state,buffer='';const reader=events.body.getReader();
  pump=(async()=>{try{while(true){const{value,done}=await reader.read();if(done)return;buffer+=new TextDecoder().decode(value);let end;while((end=buffer.indexOf('\n\n'))>=0){const event=buffer.slice(0,end);buffer=buffer.slice(end+2);if(event.startsWith('data: '))state=JSON.parse(event.slice(6));}}}catch{}})();
  for(let i=0;i<100&&!state?.connected;i++)await delay(50);
  assert.equal(state?.title,mockState.title,JSON.stringify(state));
  assert.equal(discoveryHost,`127.0.0.1:${debugPort}`);
  assert.ok(methods.includes('Runtime.enable'));assert.ok(methods.includes('Runtime.addBinding'));assert.ok(methods.includes('Runtime.evaluate'));
  const shutdown=await fetch(base+'/command',{method:'POST',headers:{'Content-Type':'application/json','X-Overlay-Token':session.token},body:'{"command":"shutdown"}'});
  assert.equal(shutdown.status,200);await shutdown.text();
  for(let i=0;i<100&&child.exitCode===null;i++)await delay(50);
  assert.equal(child.exitCode,0,stderr);
  console.log('Custom-port live tests passed: random web/debug ports, real HTTP + CDP/WebSocket + SSE, dynamic Host/Origin protections, token checks, shutdown; no real NCM control.');
}finally{
  streamAbort?.abort();await pump;
  if(child.exitCode===null){child.kill();await Promise.race([new Promise(resolve=>child.once('exit',resolve)),delay(3000)]);}
  for(const socket of sockets)socket.destroy();await new Promise(resolve=>debuggerServer.close(resolve));
}
