import {createRequire} from 'node:module';
import {readFileSync,mkdirSync} from 'node:fs';
import {createServer} from 'node:http';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const require=createRequire(import.meta.url);
const playwright=require(process.env.OM_PLAYWRIGHT_PATH||'playwright');
const commands=[];let state;let ncmRunning=true,connects=0;
const regularChoices=['屏蔽歌曲：Novacaine','屏蔽音乐人：Loyalemyx','屏蔽音乐人：Shiloh Dynasty','减少风格：嘻哈说唱','不感兴趣','总是推这首歌','盗版翻唱','听感不适'];
let feedbackChoices=regularChoices;
const server=createServer(async(request,response)=>{
 response.setHeader('Content-Type','application/json; charset=utf-8');
 if(request.url==='/session'){response.end(JSON.stringify({token:'test'}));return;}
 if(request.url==='/api/ncm-status'){response.end(JSON.stringify({running:ncmRunning}));return;}
 if(request.url==='/api/connect'){connects++;response.end('{"ok":true}');return;}
 if(request.url==='/command'){
   let body='';for await(const chunk of request)body+=chunk;const command=JSON.parse(body);commands.push(command);
   if(command.command==='mode')state={...state,mode:command.value};
   response.end(JSON.stringify({...state,...(command.command==='dislike'?{dislikeChoices:feedbackChoices}:{}),...(command.command==='dislikeReason'?{controlMessage:'已选择反馈选项'}:{})}));return;
 }
 const name=request.url==='/'?'index.html':request.url.slice(1);
 if(!['index.html','app.js','style.css','tokens.css'].includes(name)){response.writeHead(404);response.end();return;}
 response.setHeader('Content-Type',name.endsWith('.js')?'text/javascript; charset=utf-8':name.endsWith('.css')?'text/css; charset=utf-8':'text/html; charset=utf-8');
 response.end(readFileSync(new URL('./web/'+name,import.meta.url)));
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const browser=await playwright.chromium.launch({headless:true,channel:process.env.OM_BROWSER_CHANNEL||'chrome'});
const output=fileURLToPath(new URL('./test-output/web-modes/',import.meta.url));mkdirSync(output,{recursive:true});
try{
 for(const [width,height] of [[375,780],[760,780],[440,440],[320,320],[400,300],[320,240],[240,200]]){
  feedbackChoices=regularChoices;
  const context=await browser.newContext({viewport:{width,height},reducedMotion:'reduce'});
  await context.addInitScript(()=>{window.EventSource=class{close(){}};});
  const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.goto('http://127.0.0.1:'+server.address().port+'/');
  await page.locator('#quick-connect').waitFor({state:'visible'});
  const before=connects;page.once('dialog',d=>d.dismiss());await page.locator('#quick-connect').click();assert.equal(connects,before);
  page.once('dialog',d=>d.accept());await page.locator('#quick-connect').click();await page.waitForFunction(()=>!document.getElementById('quick-connect').disabled);assert.equal(connects,before+1);
  assert.deepEqual(await page.locator('footer>div').evaluate(e=>[...e.children].map(c=>c.id)),['settings-link','power','exit']);
  ncmRunning=false;await page.evaluate(()=>{lastNcmCheck=0;return checkNcm();});assert.equal(await page.locator('#quick-connect').isVisible(),false);ncmRunning=true;
  state={connected:true,id:'test',title:'Test',artist:'Artist',mode:'playOrder',volume:50,playing:false,canLike:true,canDislike:true,canAI:false};
  await page.evaluate(s=>render(s),state);
  assert.equal(await page.locator('#quick-connect').isVisible(),false);
  for(const mode of ['playAi','playFm','playOrder','playCycle','playOneCycle','playRandom']){
   await page.locator('#mode').selectOption(mode);await page.waitForFunction(()=>!document.getElementById('mode').disabled);
   assert.equal(commands.at(-1).command,'mode');assert.equal(commands.at(-1).value,mode);
   assert.equal(await page.getByRole('button',{name:'不喜欢 / 减少推荐',exact:true}).isEnabled(),true);
  }
  await page.getByRole('button',{name:'不喜欢 / 减少推荐',exact:true}).click();
  await page.getByRole('dialog').waitFor({state:'visible'});
  assert.equal(commands.at(-1).command,'dislike');assert.equal(commands.filter(c=>c.command==='dislikeReason').length,0);
  await page.getByRole('button',{name:'取消',exact:true}).click();await page.waitForFunction(()=>!document.getElementById('dislike').disabled);
  assert.equal(commands.at(-1).command,'dislikeCancel');
  await page.getByRole('button',{name:'不喜欢 / 减少推荐',exact:true}).click();
  await page.getByRole('button',{name:'不感兴趣',exact:true}).click();await page.waitForFunction(()=>!document.getElementById('dislike').disabled);
  assert.equal(commands.at(-1).command,'dislikeReason');assert.equal(commands.at(-1).value,regularChoices.indexOf('不感兴趣'));
  await page.locator('#mode').selectOption('playFm');await page.waitForFunction(()=>!document.getElementById('mode').disabled);
  await page.getByRole('button',{name:'不喜欢 / 减少推荐',exact:true}).click();
  await page.getByRole('dialog').waitFor({state:'visible'});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
  const geometry=await page.evaluate(()=>{
   const dialog=document.getElementById('dislike-dialog'),choices=document.getElementById('dislike-choices');
   const rect=dialog.getBoundingClientRect(),cancel=document.getElementById('dislike-cancel').getBoundingClientRect();
   return{top:rect.top,bottom:rect.bottom,left:rect.left,right:rect.right,height:rect.height,cancelTop:cancel.top,cancelBottom:cancel.bottom,scroll:choices.scrollHeight>choices.clientHeight+1,dialogScroll:dialog.scrollHeight>dialog.clientHeight+1,columns:getComputedStyle(choices).gridTemplateColumns.split(' ').length};
  });
  assert.ok(geometry.top>=0&&geometry.bottom<=height&&geometry.left>=0&&geometry.right<=width,JSON.stringify(geometry));
  assert.ok(geometry.cancelTop>=geometry.top&&geometry.cancelBottom<=geometry.bottom);
  assert.equal(geometry.dialogScroll,false,'Only choices should scroll');
  if(width>=320&&height>=300){assert.equal(geometry.scroll,false,`Eight regular choices should fit ${width}x${height}: ${JSON.stringify(geometry)}`);assert.equal(geometry.columns,2);}
  assert.equal(await page.locator('#dislike-cancel').evaluate(e=>document.activeElement===e),true,'Safe initial focus');
  assert.equal(await page.locator('#dislike-choices').evaluate(e=>e.scrollTop),0,'Every reopening starts at the first option');
  await page.screenshot({path:output+'/feedback-'+width+'x'+height+'.png',fullPage:true});
  await page.keyboard.press('Escape');await page.waitForFunction(()=>!document.getElementById('dislike').disabled);
  feedbackChoices=Array.from({length:20},(_,i)=>'屏蔽歌曲：'+i+' / '+('超长标题 mixed artist name '.repeat(4)));
  await page.getByRole('button',{name:'不喜欢 / 减少推荐',exact:true}).click();await page.getByRole('dialog').waitFor({state:'visible'});
  const cancelTop=await page.locator('#dislike-cancel').evaluate(e=>e.getBoundingClientRect().top);
  const lastChoice=page.locator('#dislike-choices button').last();await lastChoice.focus();
  const focused=await lastChoice.evaluate(e=>{const a=e.getBoundingClientRect(),b=e.parentElement.getBoundingClientRect();return a.bottom>b.top&&a.top<b.bottom;});
  assert.equal(focused,true,'Keyboard focus must reveal the last long option');
  assert.equal(await lastChoice.getAttribute('title'),feedbackChoices[19]);
  assert.equal(await page.locator('#dislike-cancel').evaluate(e=>e.getBoundingClientRect().top),cancelTop,'Cancel must stay fixed');
  assert.equal(await page.locator('#dislike-dialog').evaluate(e=>e.scrollHeight>e.clientHeight+1),false);
  await page.getByRole('button',{name:'取消',exact:true}).click();await page.waitForFunction(()=>!document.getElementById('dislike').disabled);
  assert.equal(commands.at(-1).command,'dislikeCancel');assert.deepEqual(errors,[]);
  commands.length=0;await context.close();
 }
 console.log('Web UI tests passed: seven viewports down to 240x200; eight choices fit 320x320/400x300/440x440, inner scrolling for 20 long options, fixed Cancel, safe focus, full labels, six modes and feedback actions.');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
