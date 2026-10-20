import {createRequire} from 'node:module';import {readFileSync,mkdirSync} from 'node:fs';import {createServer} from 'node:http';import assert from 'node:assert/strict';
const require=createRequire(import.meta.url),{chromium}=require(process.env.OM_PLAYWRIGHT_PATH||'playwright');
let current={webPort:17890,debugPort:9222,fallback:true,ncmPath:'',autoDebug:false};const requests=[];
const server=createServer(async(req,res)=>{
 res.setHeader('Content-Type','application/json');
 if(req.url==='/session'){res.end('{"token":"test"}');return;}
 if(req.url==='/api/settings'&&req.method==='GET'){res.end(JSON.stringify(current));return;}
 if(req.method==='POST'){let body='';for await(const c of req)body+=c;const value=JSON.parse(body);requests.push({path:req.url,value});if(req.url==='/api/settings')current=value;res.end(JSON.stringify(req.url==='/api/detect'?{path:'C:\\CloudMusic\\cloudmusic.exe'}:{ok:true,url:`http://127.0.0.1:${server.address().port}/settings`}));return;}
 const file=req.url==='/settings'?'settings.html':req.url.slice(1);if(!['settings.html','settings.js','style.css','tokens.css'].includes(file)){res.writeHead(404);res.end();return;}
 res.setHeader('Content-Type',file.endsWith('.html')?'text/html; charset=utf-8':file.endsWith('.js')?'text/javascript; charset=utf-8':'text/css; charset=utf-8');res.end(readFileSync(new URL('../web/'+file,import.meta.url)));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const browser=await chromium.launch({headless:true,channel:process.env.OM_BROWSER_CHANNEL||'chrome'});mkdirSync(new URL('../test-output/settings/',import.meta.url),{recursive:true});
try{for(const [width,height] of [[320,480],[440,440],[760,600]]){
 const context=await browser.newContext({viewport:{width,height},reducedMotion:'reduce'}),page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(`http://127.0.0.1:${server.address().port}/settings`);await page.waitForFunction(()=>!document.getElementById('save').disabled);
 await page.getByLabel('网页端口',{exact:true}).fill('0');await page.getByRole('button',{name:'保存',exact:true}).click();assert.equal(await page.getByLabel('网页端口',{exact:true}).getAttribute('aria-invalid'),'true');
 await page.getByLabel('网页端口',{exact:true}).fill('9222');await page.getByRole('button',{name:'保存',exact:true}).click();assert.match(await page.locator('#debug-error').textContent(),/不能相同/);
 await page.getByLabel('网页端口',{exact:true}).fill('17890');await page.getByRole('button',{name:'查找',exact:true}).click();await page.waitForFunction(()=>!document.getElementById('save').disabled);assert.equal(await page.getByLabel('网易云路径',{exact:true}).inputValue(),'C:\\CloudMusic\\cloudmusic.exe');
 const toggle=page.getByRole('switch',{name:'媒体键无响应时兜底'});await toggle.focus();await page.keyboard.press('Space');await page.getByRole('button',{name:'保存',exact:true}).click();await page.waitForFunction(()=>!document.getElementById('save').disabled);assert.equal(requests.at(-1).value.fallback,await toggle.isChecked());
 const count=requests.length;page.once('dialog',dialog=>dialog.dismiss());await page.getByRole('button',{name:'启动 / 重连网易云',exact:true}).click();assert.equal(requests.length,count,'Cancelling must not restart NCM');
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await page.screenshot({path:new URL(`../test-output/settings/${width}x${height}.png`,import.meta.url).pathname.replace(/^\/([A-Za-z]:)/,'$1'),fullPage:true});assert.deepEqual(errors,[]);await context.close();
}console.log('Settings UI passed: compact layouts, field validation, keyboard switches, path lookup, save, restart cancellation and no overflow.');}finally{await browser.close();await new Promise(r=>server.close(r));}
