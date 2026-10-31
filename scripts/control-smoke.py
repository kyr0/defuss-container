#!/usr/bin/env python3
"""Exercise actual daemon/CLI/HTTP processes without requiring virtualization."""
import json, os, pathlib, re, signal, socket, subprocess, tempfile, time, urllib.error, urllib.request
ROOT=pathlib.Path(__file__).resolve().parents[1]
def main():
 # VERIFIED: with CARGO_TARGET_DIR (or build.target-dir) set, binaries are not in ./target; ask Cargo.
 target=pathlib.Path(json.loads(subprocess.check_output(['cargo','metadata','--format-version','1','--no-deps','--manifest-path',str(ROOT/'Cargo.toml')]))['target_directory'])/'debug'
 suffix='.exe' if os.name=='nt' else '';binary=target/('defuss-container'+suffix);daemon=target/('defuss-containerd'+suffix)
 with tempfile.TemporaryDirectory(prefix='defuss-control-') as work:
  root=pathlib.Path(work).resolve();home=root/'state';log=(root/'daemon.log').open('wb')
  # No RUST_LOG, like the installed services: the daemon must still log at info level.
  env={k:v for k,v in os.environ.items() if k!='RUST_LOG'}
  proc=subprocess.Popen([str(daemon),'--home',str(home),'--engine',str(root/'missing-engine'),'--port','0'],stdout=log,stderr=log,env=env)
  try:
   for _ in range(200):
    if (home/'connection.json').exists():break
    if proc.poll() is not None:raise RuntimeError('daemon exited: '+(root/'daemon.log').read_text())
    time.sleep(.025)
   connection=json.loads((home/'connection.json').read_text());base=connection['address']
   def cli(*args,check=True):
    result=subprocess.run([str(binary),'--home',str(home),*args],capture_output=True,text=True,check=check)
    return json.loads(result.stdout) if check else result
   def http(path,auth=True,origin=None):
    headers={'Authorization':'Bearer '+connection['token']} if auth else {}
    if origin:headers['Origin']=origin
    req=urllib.request.Request(base+path,headers=headers)
    try:
     with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req,timeout=5) as response:return response.status,response.read()
    except urllib.error.HTTPError as e:return e.code,e.read()
   assert http('/v1/health')[0]==200
   assert re.search(r'^\d{4}-\d\d-\d\dT[\d:.]+Z +INFO .*daemon listening',(root/'daemon.log').read_text(),re.M),'daemon logs ISO-8601 + level by default'
   assert http('/v1/apps',False)[0]==401
   assert http('/v1/apps',True,'https://evil.example')[0]==403
   assert cli('list')==[]
   spec={'name':'hello','repository':'https://github.com/example/hello.git','mounts':str(root/'shares')}
   file=root/'app.json';file.write_text(json.dumps(spec));assert cli('create',str(file))['spec']['name']=='hello'
   assert cli('create',str(file),check=False).returncode!=0
   assert len(cli('list'))==1
   job=cli('build','hello');finished=cli('wait',job['id'],'--timeout','10',check=False);assert finished.returncode!=0
   stored=cli('job',job['id']);assert stored['state']=='failed' and 'launch child process' in stored['error']
   assert cli('delete','hello','--confirm','wrong',check=False).returncode!=0
   assert (root/'shares/input').is_dir() and (root/'shares/output').is_dir() and (root/'shares/etc').is_dir()
   duplicate=subprocess.run([str(daemon),'--home',str(home),'--port','0'],capture_output=True,timeout=5);assert duplicate.returncode!=0
   held=socket.socket();held.bind(('127.0.0.1',0));held.listen();busy=subprocess.run([str(daemon),'--home',str(root/'state-busy'),'--engine',str(root/'missing-engine'),'--port',str(held.getsockname()[1])],capture_output=True,timeout=10);held.close()
   assert busy.returncode!=0 and b'DEFUSS_CONTAINER_PORT' in busy.stderr,'a taken port fails fast and names the override'
   if os.name!='nt':assert (home/'connection.json').stat().st_mode&0o077==0
   old_token=connection['token'];proc.terminate();proc.wait(timeout=10)
   proc=subprocess.Popen([str(daemon),'--home',str(home),'--engine',str(root/'missing-engine'),'--port','0'],stdout=log,stderr=log)
   for _ in range(200):
    if proc.poll() is not None:raise RuntimeError('restarted daemon exited')
    try:
     updated=json.loads((home/'connection.json').read_text())
     if updated['token']!=old_token:break
    except (FileNotFoundError,json.JSONDecodeError):pass
    time.sleep(.025)
   else:raise RuntimeError('daemon did not rotate credentials')
   connection=updated;base=connection['address']
   assert http('/v1/health')[0]==200 and len(cli('list'))==1
   connection={**updated,'token':old_token};assert http('/v1/health')[0]==401;connection=updated
   print(json.dumps({'passed':17,'scope':'actual daemon, CLI, HTTP auth, job failure, database, single-instance lock, taken port, restart and token rotation; no VM'},indent=2))
  finally:
   proc.terminate()
   try:proc.wait(timeout=10)
   except subprocess.TimeoutExpired:proc.kill();proc.wait()
   log.close()
if __name__=='__main__':main()
