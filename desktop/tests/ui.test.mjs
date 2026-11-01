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
// VERIFIED: an installed Playwright 1.56 clock keeps running in real time (its injected controller
// calls resume()) and catches up at least every 100 ms, so a slow runner fired the 2 s retry early (CI macOS, 3 !== 2).
// pauseAt() freezes it: the page starts at tick 0 and timers fire only through runFor.
async function page(t,offline=false,{clock=false}={}){const p=await browser.newPage({viewport:{width:1365,height:1000}});t.after(()=>p.close());if(clock)await p.clock.pauseAt(Date.now());await p.addInitScript(({offline})=>{
 const app={spec:{name:'hello',repository:'https://github.com/example/hello.git',git_ref:'main',memory_mib:1024,cpus:2,mounts:'/home/user/shares',ports:[{host:8080,guest:3000}]},active_release:'11111111-1111-4111-8111-111111111111',previous_release:null,desired_running:true,last_error:null};
 window.__requests=[];window.__commands=new Set();window.__attempts=0;window.__offline=offline;const apps=[app],jobs=[];
 window.__TAURI_INTERNALS__={invoke:async(command,args)=>{window.__commands.add(command);if(args?.path==='/v1/apps'&&args.method==='GET')window.__attempts++;if(window.__offline)throw new Error('Daemon unavailable');if(command!=='plugin:defuss-container|daemon_request')throw new Error('unexpected native command');window.__requests.push(args);const {method,path,body}=args;
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
test('offline shell renders recovery instructions and a header Reconnect button',async t=>{const p=await page(t,true);assert.ok((await p.locator('#notice').innerText()).includes('Start the daemon'));await p.locator('header').getByRole('button',{name:'Reconnect',exact:true}).waitFor();});
test('offline UI retries after 1, 2, 4 s and reconnects without a click',async t=>{const p=await page(t,true,{clock:true});const attempts=()=>p.evaluate(()=>window.__attempts);
 assert.equal(await attempts(),1);await p.clock.runFor(999);assert.equal(await attempts(),1);await p.clock.runFor(1);await p.waitForFunction(()=>window.__attempts===2);
 await p.clock.runFor(1999);assert.equal(await attempts(),2);await p.clock.runFor(1);await p.waitForFunction(()=>window.__attempts===3);
 await p.evaluate(()=>{window.__offline=false;});await p.clock.runFor(3999);assert.equal(await attempts(),3);await p.clock.runFor(1);
 await p.getByText('Daemon connected',{exact:true}).waitFor();assert.equal(await p.locator('#reconnect').isVisible(),false);});
test('Reconnect retries at once and resets the backoff',async t=>{const p=await page(t,true,{clock:true});await p.clock.runFor(1000);await p.waitForFunction(()=>window.__attempts===2);
 await p.getByRole('button',{name:'Reconnect',exact:true}).click();await p.waitForFunction(()=>window.__attempts===3);await p.clock.runFor(1000);await p.waitForFunction(()=>window.__attempts===4);});
// VERIFIED: the packaged UI is a remote origin (http://127.0.0.1:<port>) to Tauri, which rejected an ungranted command
// with "not allowed by ACL"; every command the built UI invokes must be a plugin command granted to that origin.
test('every native command the UI invokes is granted to the loopback UI origin',async t=>{const p=await page(t);const commands=await p.evaluate(()=>[...window.__commands]);assert.ok(commands.length>0);
 const here=path.dirname(fileURLToPath(import.meta.url));const config=JSON.parse(fs.readFileSync(path.join(here,'../defuss-tauri.json'),'utf8'));const native=path.join(here,'../native');
 const capabilities=config.rust.extensions.flatMap(e=>e.capabilities??[]);const links=fs.readFileSync(path.join(native,'Cargo.toml'),'utf8');const build=fs.readFileSync(path.join(native,'build.rs'),'utf8');
 for(const command of commands){const m=/^plugin:([a-z-]+)\|([a-z_]+)$/.exec(command);assert.ok(m,`${command} is a plugin command`);const [,plugin,name]=m;
  assert.ok(links.includes(`links = "tauri-plugin-${plugin}"`),`native crate links tauri-plugin-${plugin}`);assert.ok(build.includes(`"${name}"`),`build.rs generates the ${name} permission`);
  assert.ok(capabilities.some(c=>c.windows?.includes('main')&&c.remote?.urls?.includes('http://127.0.0.1:*')&&c.permissions?.includes(`${plugin}:allow-${name.replaceAll('_','-')}`)),`${command} granted to http://127.0.0.1:* in the main window`);}});
test('dashboard remains within a narrow viewport',async t=>{const p=await page(t);await p.setViewportSize({width:760,height:1000});assert.ok(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));});
test('optional visual snapshot',async t=>{const p=await page(t);if(process.env.UI_SCREENSHOT)await p.screenshot({path:process.env.UI_SCREENSHOT,fullPage:true});});
test('application name field enforces its pattern in the browser',async t=>{const p=await page(t);const errors=[];p.on('console',m=>{if(m.type()==='error')errors.push(m.text());});await p.getByRole('button',{name:'+ Add application',exact:true}).click();const name=p.getByLabel('Name',{exact:true});
 for(const [value,mismatch] of [['Bad_Name',true],['-x',true],['good-name-2',false]]){await name.fill(value);assert.equal(await name.evaluate(e=>e.validity.patternMismatch),mismatch,value);}
 assert.deepEqual(errors,[]);});
