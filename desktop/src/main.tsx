import { render } from 'defuss';
import { invoke } from '@tauri-apps/api/core';
import './style.css';

type App={spec:{name:string;repository:string;git_ref:string;memory_mib:number;cpus:number;mounts:string;ports:{host:number;guest:number}[]};active_release:string|null;previous_release:string|null;desired_running:boolean;last_error:string|null};
type Job={id:string;app:string;state:string;action:{action:string};error:string|null;result:unknown};
type Release={id:string;commit:string;created_at:number};
type Backup={id:string;created_at:number;bytes:number};
let apps:App[]=[];let jobs:Job[]=[];let selected:string|null=null;let online=false;let pending=false;let refreshing=false;
const $=(id:string)=>document.getElementById(id)!;
const pretty=(v:unknown)=>typeof v==='string'?v:JSON.stringify(v,null,2);
async function api<T>(method:string,path:string,body?:unknown):Promise<T>{return invoke<T>('daemon_request',{method,path,body:body??null});}
function message(text:string,error=false){$('notice').textContent=text;$('notice').className=error?'notice error':'notice';}
function details(v:unknown){$('detail-output').textContent=pretty(v);($('detail') as HTMLDialogElement).showModal();}
function busy(name:string){return jobs.some(j=>j.app===name&&['queued','running'].includes(j.state));}
async function action(name:string,action:string,extra:Record<string,unknown>={}){
  try{const j=await api<Job>('POST',`/v1/apps/${name}/actions`,{action,...extra});message(`${action} queued · ${j.id}`);await refresh();}
  catch(e){message(String(e),true);}
}
async function refresh(){
  if(refreshing)return;refreshing=true;
  try{[apps,jobs]=await Promise.all([api<App[]>('GET','/v1/apps'),api<Job[]>('GET','/v1/jobs')]);if(!online)message('Connected to the local daemon.');online=true;
    $('connection').textContent='Daemon connected';$('connection').className='connection online';
    if(!selected&&apps.length)selected=apps[0].spec.name;
    paint();
  }catch(e){online=false;$('connection').textContent='Daemon unavailable';$('connection').className='connection';message(`${e}. Start the installed daemon service, then reconnect.`,true);}
  finally{refreshing=false;}
}
function paint(){
  const running=apps.filter(a=>a.desired_running).length;
  $('count').textContent=String(apps.length);$('running-count').textContent=String(running);$('job-count').textContent=String(jobs.filter(j=>['running','queued'].includes(j.state)).length);
  const list=$('app-list');list.replaceChildren();
  render(<>{apps.length?apps.map(a=><button className={`app-item ${selected===a.spec.name?'selected':''}`} onClick={()=>{selected=a.spec.name;paint();}}><span className="app-icon">B</span><span><strong>{a.spec.name}</strong><small>{a.spec.cpus} vCPU · {a.spec.memory_mib} MiB</small></span><span className={`dot ${a.desired_running?'green':''}`}></span></button>):<div className="empty">No applications yet.<br/>Add a repository to begin.</div>}</>,list);
  const panel=$('application');panel.replaceChildren();const a=apps.find(a=>a.spec.name===selected);
  if(a){const disabled=busy(a.spec.name);render(<>
    <div className="panel-heading"><div><div className="eyebrow">APPLICATION</div><h2>{a.spec.name}</h2></div><span className={`badge ${a.desired_running?'live':''}`}>{a.desired_running?'Desired: running':'Desired: stopped'}</span></div>
    <p className="repository">{a.spec.repository}</p><div className="metadata"><span>Ref <b>{a.spec.git_ref}</b></span><span>Release <b>{a.active_release?.slice(0,8)||'None'}</b></span><span>Storage <b>Guest-local ext4</b></span></div>
    {a.last_error?<pre className="inline-error">{a.last_error}</pre>:null}
    <div className="actions"><button className="primary" disabled={disabled} onClick={()=>action(a.spec.name,'build')}>Build release</button><button disabled={disabled||!a.active_release} onClick={()=>action(a.spec.name,a.desired_running?'stop':'start')}>{a.desired_running?'Stop':'Start'}</button><button disabled={disabled||!a.previous_release} onClick={()=>action(a.spec.name,'rollback')}>Rollback</button><button disabled={disabled||!a.active_release} onClick={()=>action(a.spec.name,'backup')}>Back up data</button></div>
    <div className="links"><button onClick={()=>inspect(a.spec.name,'')}>Live status</button><button onClick={()=>inspect(a.spec.name,'/logs')}>Logs</button><button onClick={()=>showReleases(a.spec.name)}>Releases / deploy</button><button onClick={()=>showBackups(a.spec.name)}>Backups / restore</button><button className="danger-text" disabled={disabled} onClick={()=>deleteApp(a.spec.name)}>Delete machine</button></div>
    <div className="storage"><h3>Host shares</h3><code>{a.spec.mounts}</code><p>input · read-only &nbsp; / &nbsp; output · writable &nbsp; / &nbsp; etc · read-only</p><p>Application data persists inside the VM. Backups export <code>/app/var</code>.</p></div>
  </>,panel);}else render(<div className="welcome"><span className="hero-icon">◈</span><h2>Your backend, locally.</h2><p>Build a Bun repository into a distroless image.<br/>Run it in its own Linux machine.</p><button className="primary" onClick={()=>($('create') as HTMLDialogElement).showModal()}>Add application</button></div>,panel);
  const activity=$('activity');activity.replaceChildren();render(<>{jobs.slice(0,12).map(j=><button className="job" onClick={async()=>{try{details({job:j,log:await api('GET',`/v1/jobs/${j.id}/log`)});}catch(e){message(String(e),true);}}}><span className={`job-state ${j.state}`}>{j.state==='succeeded'?'✓':j.state==='failed'?'!':'·'}</span><span><strong>{j.app}</strong><small>{j.action.action}</small></span><span className="badge">{j.state}</span></button>)}{!jobs.length?<p className="muted">Builds, deployments and backups appear here.</p>:null}</>,activity);
}
async function inspect(name:string,suffix:string){try{details(await api('GET',`/v1/apps/${name}${suffix}`));}catch(e){message(String(e),true);}}
function choiceDialog(title:string,rows:{title:string;subtitle:string;label:string;run:()=>void}[]){
  $('choice-title').textContent=title;const target=$('choices');target.replaceChildren();
  render(<>{rows.map(r=><div className="choice"><div><strong>{r.title}</strong><small>{r.subtitle}</small></div><button className="primary" onClick={()=>{($('choice') as HTMLDialogElement).close();r.run();}}>{r.label}</button></div>)}{!rows.length?<p>No entries yet.</p>:null}</>,target);($('choice') as HTMLDialogElement).showModal();
}
async function showReleases(name:string){try{const r=await api<Release[]>('GET',`/v1/apps/${name}/releases`);choiceDialog('Deploy a release',r.map(v=>({title:v.id,subtitle:`Commit ${v.commit.slice(0,12)} · ${new Date(v.created_at*1000).toLocaleString()}`,label:'Deploy',run:()=>action(name,'deploy',{release:v.id})})));}catch(e){message(String(e),true);}}
async function showBackups(name:string){try{const b=await api<Backup[]>('GET',`/v1/apps/${name}/backups`);choiceDialog('Restore application data',b.map(v=>({title:v.id,subtitle:`${new Date(v.created_at*1000).toLocaleString()} · ${(v.bytes/1048576).toFixed(1)} MiB. Restore replaces data; a safety backup is made first.`,label:'Restore',run:()=>{if(confirm('Replace current application data with this backup?'))void action(name,'restore',{backup:v.id});}})));}catch(e){message(String(e),true);}}
function deleteApp(name:string){const entered=prompt(`This permanently deletes ${name}'s VM and guest data. Host shares and backups remain. Type ${name} to continue.`);if(entered===name)void action(name,'delete',{confirm:entered});}
async function create(event:Event){event.preventDefault();if(pending)return;pending=true;
  const f=new FormData($('create-form') as HTMLFormElement);const value=(k:string)=>String(f.get(k)||'');
  try { const spec={name:value('name'),repository:value('repository'),git_ref:value('git_ref')||'HEAD',subdirectory:value('subdirectory'),mounts:value('mounts'),cpus:Number(value('cpus')),memory_mib:Number(value('memory')),storage_gib:20,ports:[{host:Number(value('host_port')),guest:Number(value('guest_port'))}],env:JSON.parse(value('env')||'{}'),build:f.has('build'),health:{port:Number(value('guest_port')),path:value('health')||'/health'},stop_timeout_seconds:30};
  await api('POST','/v1/apps',spec);selected=spec.name;($('create') as HTMLDialogElement).close();message(`Added ${spec.name}. Build its first release.`);await refresh();}catch(e){$('form-error').textContent=String(e);}finally{pending=false;}
}
render(<div className="shell"><aside><div className="brand"><span>◈</span> defuss<span className="brand-light">container</span></div><div className="workspace-label">LOCAL WORKSPACE</div><button className="nav active">▦ &nbsp; Applications</button><button className="nav" onClick={async()=>{try{details(await api('GET','/v1/doctor'));}catch(e){message(String(e),true);}}}>⌘ &nbsp; Runtime diagnostics</button><div className="sidebar-bottom"><span className="version">v0.1.0 · Bun 1.4.4</span><span id="connection" className="connection">Connecting…</span><button className="reconnect" onClick={refresh}>Reconnect</button></div></aside><main>
  <header><div><div className="eyebrow">RUNTIME CONTROL</div><h1>Applications</h1><p>Isolated machines. Persistent data. One local control plane.</p></div><button className="primary" onClick={()=>($('create') as HTMLDialogElement).showModal()}>+ Add application</button></header>
  <div id="notice" className="notice" role="status" aria-live="polite"></div>
  <div className="stats"><div><small>Applications</small><strong id="count">0</strong></div><div><small>Desired running</small><strong id="running-count">0</strong></div><div><small>Active operations</small><strong id="job-count">0</strong></div><div><small>Runtime</small><strong className="text-stat">smolvm <span>1.32.4</span></strong></div></div>
  <div className="workspace"><section className="app-list" id="app-list"></section><section className="panel" id="application"></section></div>
  <section className="activity-panel"><div className="section-heading"><h3>Recent activity</h3><span>Operations continue when this window closes</span></div><div id="activity"></div></section>
</main><dialog id="create"><form id="create-form" onSubmit={create}><div className="dialog-heading"><h2>Add application</h2><button type="button" onClick={()=>($('create') as HTMLDialogElement).close()}>×</button></div><p>Repository must contain a Bun lockfile and start / stop scripts.</p><div className="form-grid">
  <label>Name<input name="name" required pattern="[a-z][a-z0-9-]{0,47}" placeholder="my-api"/></label><label>Git ref<input name="git_ref" value="HEAD"/></label>
  <label className="wide">HTTPS Git URL<input name="repository" required type="url" placeholder="https://github.com/your-org/your-api.git"/></label>
  <label className="wide">Absolute host share directory<input name="mounts" required placeholder="/Users/you/defuss/my-api"/></label>
  <label>Repository subdirectory<input name="subdirectory" placeholder="Root by default"/></label><label>Health path<input name="health" value="/health"/></label>
  <label>Host port<input name="host_port" type="number" min="1024" max="65535" value="8080" required/></label><label>Guest port<input name="guest_port" type="number" min="1" max="65535" value="3000" required/></label>
  <label>vCPUs<input name="cpus" type="number" min="1" max="64" value="2" required/></label><label>Memory / MiB<input name="memory" type="number" min="256" max="262144" value="1024" required/></label>
  <label className="wide">Environment / JSON<textarea name="env" rows={3}>{'{"PORT":"3000"}'}</textarea></label><label className="checkbox wide"><input name="build" type="checkbox"/> Run package build script</label>
</div><p className="muted">Published ports use the engine’s host listener. Apply host firewall rules if access must be limited.</p><pre id="form-error" className="inline-error"></pre><button type="submit" className="primary">Create application</button></form></dialog>
<dialog id="detail"><div className="dialog-heading"><h2>Details</h2><button onClick={()=>($('detail') as HTMLDialogElement).close()}>Close</button></div><pre id="detail-output"></pre></dialog><dialog id="choice"><div className="dialog-heading"><h2 id="choice-title"></h2><button onClick={()=>($('choice') as HTMLDialogElement).close()}>Close</button></div><div id="choices"></div></dialog></div>,document.getElementById('app'));
paint();void refresh();setInterval(()=>{if(online&&!document.hidden)void refresh();},3000);
