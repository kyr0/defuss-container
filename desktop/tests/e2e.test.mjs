// GUI e2e: the built UI (dist/) in a real Playwright browser, talking to the live daemon that tests/e2e/run.py
// started. The Tauri IPC is replaced by a binding that runs `defuss-container api`, the same Rust client the native
// bridge calls, so every click reaches the real daemon and a real VM. Run through tests/e2e/run.py, not directly.
import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import playwright from 'playwright';

const env=name=>{const v=process.env[name];if(!v)throw new Error(`${name} unset: run tests/e2e/run.py`);return v;};
const CLI=env('E2E_CLI'),HOME=env('E2E_HOME'),APP=env('E2E_APP'),PORT=Number(env('E2E_APP_PORT')),DIST=env('E2E_DIST'),OUT=env('E2E_OUTPUT');
const SECOND='gui-second';
const run=promisify(execFile);
async function cli(...args){const {stdout}=await run(CLI,['--home',HOME,...args],{maxBuffer:64*1024*1024});return JSON.parse(stdout);}
async function api(method,p,body){
  const args=['api',method,p];if(body!=null)args.push('--body',JSON.stringify(body));
  try{return await cli(...args);}catch(e){throw new Error((e.stderr||String(e)).trim());}
}
async function health(){try{const r=await fetch(`http://127.0.0.1:${PORT}/health`,{signal:AbortSignal.timeout(3000)});return r.ok;}catch{return false;}}
async function settled(id){
  for(let i=0;i<900;i++){const j=await cli('job',id);if(!['queued','running'].includes(j.state))return j;await new Promise(r=>setTimeout(r,1000));}
  throw new Error(`job ${id} did not finish`);
}

let browser,server,page,base;const problems=[];
before(async()=>{
  fs.mkdirSync(OUT,{recursive:true});
  const root=path.resolve(DIST);
  server=http.createServer((req,res)=>{const rel=decodeURIComponent(new URL(req.url,'http://x').pathname);const file=path.join(root,rel==='/'?'index.html':rel);
    if(!file.startsWith(root+path.sep)){res.writeHead(403);res.end();return;}
    try{res.setHeader('Content-Type',file.endsWith('.js')?'application/javascript':file.endsWith('.css')?'text/css':'text/html');res.end(fs.readFileSync(file));}catch{res.writeHead(404);res.end();}});
  await new Promise(r=>server.listen(0,'127.0.0.1',r));base=`http://127.0.0.1:${server.address().port}`;
  const engine=process.env.E2E_BROWSER||'chromium';
  browser=await playwright[engine].launch({headless:true,...(engine==='chromium'?{args:['--no-sandbox']}:{})});
  const context=await browser.newContext({viewport:{width:1365,height:1000}});
  await context.exposeFunction('__defussDaemonRequest',({method,path:p,body})=>api(method,p,body));
  await context.addInitScript(()=>{window.__TAURI_INTERNALS__={invoke:(command,args)=>command==='plugin:defuss-container|daemon_request'?window.__defussDaemonRequest(args):Promise.reject(new Error(`unexpected native command ${command}`))};});
  page=await context.newPage();
  page.on('pageerror',e=>problems.push(`pageerror: ${e.message}`));
  page.on('console',m=>{if(m.type()==='error')problems.push(`console: ${m.text()}`);});
  page.on('requestfailed',r=>problems.push(`requestfailed: ${r.url()}`));
  await page.goto(base);
  await page.getByText('Daemon connected',{exact:true}).waitFor({timeout:30000});
});
after(async()=>{await browser?.close();await new Promise(r=>server?.close(r));});

const shot=name=>page.screenshot({path:path.join(OUT,`${name}.png`),fullPage:true});
// The notice changes once the action request returns: a queued job id, or the daemon's error.
async function act(target){
  const before=await page.locator('#notice').innerText();
  await target.click();
  await page.waitForFunction(b=>{const t=document.querySelector('#notice').textContent;return t!==b&&/queued · |rror/.test(t);},before,{timeout:30000});
  const text=await page.locator('#notice').innerText();const m=/queued · ([0-9a-f-]{36})/.exec(text);assert.ok(m,`no queued job in notice: ${text}`);
  const job=await settled(m[1]);assert.equal(job.state,'succeeded',`${text}: ${job.error}`);return job;
}
const click=name=>act(page.getByRole('button',{name,exact:true}));
async function select(name){await page.locator('.app-item',{hasText:name}).click();await page.getByRole('heading',{name,exact:true}).waitFor();}
async function dialogText(){const d=page.locator('#detail[open]');await d.waitFor();const text=await page.locator('#detail-output').innerText();await d.getByRole('button',{name:'Close'}).click();return text;}

test('dashboard shows the deployed app from the live daemon',async()=>{
  const shown=await cli('show',APP);
  await select(APP);
  await page.getByText('Desired: running',{exact:true}).waitFor();
  assert.equal(await page.locator('.metadata b').nth(1).innerText(),shown.application.active_release.slice(0,8));
  assert.ok((await page.locator('.repository').innerText()).startsWith('https://127.0.0.1:'));
  await shot('dashboard');
});
test('runtime diagnostics report a ready hypervisor',async()=>{
  await page.getByRole('button',{name:/Runtime diagnostics/}).click();
  const text=await dialogText();assert.match(text,/"hypervisor_ready": true/);assert.match(text,/"version": "smolvm 1\.32\.4"/);
});
test('live status and logs come from the VM',async()=>{
  await page.getByRole('button',{name:'Live status',exact:true}).click();
  assert.match(await dialogText(),/"state": "running"/i);
  await page.getByRole('button',{name:'Logs',exact:true}).click();
  const logs=await dialogText();assert.match(logs,/defuss-supervisor started/);assert.match(logs,/hello listening/);
});
test('stop and start buttons control the workload',async()=>{
  await click('Stop');assert.equal(await health(),false);
  await page.getByRole('button',{name:'Start',exact:true}).waitFor({timeout:15000});
  await click('Start');assert.equal(await health(),true);
  await page.getByText('Desired: running',{exact:true}).waitFor({timeout:15000});
});
test('back up, then restore through the backups dialog',async()=>{
  await page.getByRole('button',{name:'Back up data',exact:true}).waitFor({state:'visible'});
  await page.waitForFunction(()=>!document.querySelector('.actions button')?.disabled,null,{timeout:15000});
  const backup=await click('Back up data');
  await page.getByRole('button',{name:'Backups / restore',exact:true}).click();
  const row=page.locator('#choices .choice',{hasText:backup.result.id});await row.waitFor();await shot('backups');
  page.once('dialog',d=>d.accept());
  await act(row.getByRole('button',{name:'Restore',exact:true}));assert.equal(await health(),true);
});
test('deploy the active release from the releases dialog',async()=>{
  const active=(await cli('show',APP)).application.active_release;
  await page.waitForFunction(()=>!document.querySelector('.actions button')?.disabled,null,{timeout:15000});
  await page.getByRole('button',{name:'Releases / deploy',exact:true}).click();
  const row=page.locator('#choices .choice',{hasText:active});await row.waitFor();
  assert.ok(await page.locator('#choices .choice').count()>=2,'every built release is listed');
  await act(row.getByRole('button',{name:'Deploy',exact:true}));assert.equal(await health(),true);
});
test('activity entries open the job record with its log',async()=>{
  await page.locator('.job').first().click();
  const text=await dialogText();assert.match(text,/"job":/);assert.match(text,/"log":/);
});
test('add an application through the form, then delete it with typed confirmation',async()=>{
  await page.getByRole('button',{name:'+ Add application',exact:true}).click();
  await page.getByLabel('Name',{exact:true}).fill(SECOND);
  await page.getByLabel('Git ref').fill('main');
  await page.getByLabel('HTTPS Git URL').fill(env('E2E_REPOSITORY'));
  await page.getByLabel('Absolute host share directory').fill(path.join(env('E2E_SHARES'),SECOND));
  await page.getByLabel('Repository subdirectory').fill('apps/hello');
  await page.getByLabel('Host port').fill(env('E2E_SECOND_PORT'));
  await page.getByRole('button',{name:'Create application',exact:true}).click();
  await page.getByRole('heading',{name:SECOND,exact:true}).waitFor();
  const created=await cli('show',SECOND);assert.equal(created.application.spec.subdirectory,'apps/hello');
  assert.ok(fs.statSync(path.join(env('E2E_SHARES'),SECOND,'output')).isDirectory(),'daemon created the shares');
  await shot('second-app');
  page.once('dialog',d=>d.accept(SECOND));
  await click('Delete machine');
  await page.locator('.app-item',{hasText:SECOND}).waitFor({state:'detached',timeout:15000});
  assert.deepEqual((await cli('list')).map(a=>a.spec.name),[APP]);
});
test('no console errors, page errors or failed requests',()=>{assert.deepEqual(problems,[]);});
