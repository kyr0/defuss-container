import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../dist');
let browser,server,address;
before(async()=>{
 server=http.createServer((req,res)=>{const rel=decodeURIComponent(new URL(req.url,'http://localhost').pathname);const file=path.join(root,rel==='/'?'index.html':rel);if(!file.startsWith(root+path.sep)){res.writeHead(403);res.end();return;}try{res.setHeader('Content-Type',file.endsWith('.js')?'application/javascript':file.endsWith('.css')?'text/css':'text/html');res.end(fs.readFileSync(file));}catch{res.writeHead(404);res.end();}});
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));address=`http://127.0.0.1:${server.address().port}`;
 browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_EXECUTABLE||undefined,args:['--no-sandbox','--disable-dev-shm-usage']});
});
after(async()=>{await browser?.close();await new Promise(resolve=>server?.close(resolve));});
async function page(t,offline=false){const p=await browser.newPage({viewport:{width:1365,height:1000}});t.after(()=>p.close());await p.addInitScript(({offline})=>{
 const app={spec:{name:'hello',repository:'https://github.com/example/hello.git',git_ref:'main',memory_mib:1024,cpus:2,mounts:'/home/user/shares',ports:[{host:8080,guest:3000}]},active_release:'11111111-1111-4111-8111-111111111111',previous_release:null,desired_running:true,last_error:null};
 window.__requests=[];const apps=[app],jobs=[];
 window.__TAURI_INTERNALS__={invoke:async(command,args)=>{if(offline)throw new Error('Daemon unavailable');if(command!=='daemon_request')throw new Error('unexpected native command');window.__requests.push(args);const {method,path,body}=args;
 if(path==='/v1/apps'&&method==='GET')return apps;
 if(path==='/v1/apps'&&method==='POST'){const created={...app,spec:body,active_release:null,desired_running:false};apps.push(created);return created;}
 if(path==='/v1/jobs')return jobs;
 if(path.endsWith('/actions')){const job={id:crypto.randomUUID(),app:path.split('/')[3],state:'succeeded',action:body,error:null,result:null};jobs.unshift(job);return job;}
 if(path.endsWith('/releases'))return [{id:'11111111-1111-4111-8111-111111111111',commit:'a'.repeat(40),created_at:1793400000}];
 if(path.endsWith('/backups'))return [];
 if(path.endsWith('/logs'))return '<script>window.xss=true</script> log output';
 if(path==='/v1/doctor')return {provider:'linux-kvm',expected_engine:'1.32.4',hypervisor_ready:true};
 if(path.includes('/jobs/'))return 'build log';
 return {application:app,observed:{state:'Running'}};
 }};
 },{offline});await p.goto(address);await p.getByText(offline?'Daemon unavailable':'Daemon connected',{exact:true}).waitFor();return p;}
test('dashboard renders app and routes build action exclusively through native HTTP bridge',async t=>{const p=await page(t);await p.getByRole('heading',{name:'hello',exact:true}).waitFor();await p.getByRole('button',{name:'Build release',exact:true}).click();await p.waitForFunction(()=>window.__requests.some(r=>r.method==='POST'&&r.path==='/v1/apps/hello/actions'&&r.body.action==='build'));assert.equal(await p.locator('#count').innerText(),'1');});
test('release picker submits explicit release id',async t=>{const p=await page(t);await p.getByRole('button',{name:'Releases / deploy'}).click();await p.getByRole('button',{name:'Deploy',exact:true}).click();await p.waitForFunction(()=>window.__requests.some(r=>r.body?.action==='deploy'&&r.body.release==='11111111-1111-4111-8111-111111111111'));});
test('logs are escaped text, never executable HTML',async t=>{const p=await page(t);await p.getByRole('button',{name:'Logs',exact:true}).click();await p.locator('#detail[open]').waitFor();assert.ok((await p.locator('#detail-output').innerText()).includes('<script>'));assert.equal(await p.evaluate(()=>window.xss),undefined);});
test('invalid environment JSON remains recoverable and valid app can then be created',async t=>{const p=await page(t);await p.getByRole('button',{name:'+ Add application',exact:true}).click();await p.getByLabel('Name',{exact:true}).fill('second');await p.getByLabel('HTTPS Git URL').fill('https://github.com/example/second.git');await p.getByLabel('Absolute host share directory').fill('/home/user/second');await p.getByLabel('Environment / JSON').fill('{broken');await p.getByRole('button',{name:'Create application',exact:true}).click();await p.waitForFunction(()=>document.querySelector('#form-error').textContent.length>0);await p.getByLabel('Environment / JSON').fill('{"PORT":"3000"}');await p.getByRole('button',{name:'Create application',exact:true}).click();await p.getByRole('heading',{name:'second',exact:true}).waitFor();assert.equal(await p.locator('#count').innerText(),'2');});
test('offline shell renders recovery instructions without native server lifecycle calls',async t=>{const p=await page(t,true);assert.ok((await p.locator('#notice').innerText()).includes('Start the installed daemon service'));await p.getByRole('button',{name:'Reconnect',exact:true}).waitFor();});
test('dashboard remains within a narrow viewport',async t=>{const p=await page(t);await p.setViewportSize({width:760,height:1000});assert.ok(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));});
test('optional visual snapshot',async t=>{const p=await page(t);if(process.env.UI_SCREENSHOT)await p.screenshot({path:process.env.UI_SCREENSHOT,fullPage:true});});
