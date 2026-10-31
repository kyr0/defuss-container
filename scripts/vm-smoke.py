#!/usr/bin/env python3
"""Opt-in real-hardware gate. Requires an installed/running daemon and a reachable
HTTPS Git repository containing examples/hello (or an app implementing that HTTP contract).
Creates an app, validates deploy/host/share/backup/restart/restore/delete; never uses mocks.
"""
import argparse, json, os, pathlib, subprocess, tempfile, threading, time, urllib.request
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
class Host(BaseHTTPRequestHandler):
 def do_GET(self):self.send_response(200);self.end_headers();self.wfile.write(b'defuss-physical-host')
 def log_message(self,*args):pass
def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--repository',required=True);p.add_argument('--git-ref',default='HEAD');p.add_argument('--subdirectory',default='examples/hello');p.add_argument('--cli',default='defuss-container');p.add_argument('--port',type=int,default=38080);p.add_argument('--report',type=pathlib.Path,default=pathlib.Path('vm-smoke-report.json'));a=p.parse_args()
 name='smoke-'+str(int(time.time()));results=[]
 def cli(*args):return json.loads(subprocess.check_output([a.cli,*args],text=True))
 def operation(*args):
  j=cli(*args);result=cli('wait',j['id'],'--timeout','1800');assert result['state']=='succeeded';return result['result']
 def http(path,method='GET'):
  request=urllib.request.Request(f'http://127.0.0.1:{a.port}{path}',method=method)
  with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request,timeout=10) as r:return r.read().decode()
 host=ThreadingHTTPServer(('127.0.0.1',0),Host);threading.Thread(target=host.serve_forever,daemon=True).start()
 created=False
 with tempfile.TemporaryDirectory(prefix='defuss-vm-smoke-') as temp:
  root=pathlib.Path(temp).resolve();spec={'name':name,'repository':a.repository,'git_ref':a.git_ref,'subdirectory':a.subdirectory,'mounts':str(root/'shares'),'ports':[{'host':a.port,'guest':3000}],'env':{'PORT':'3000','HOST_TEST_PORT':str(host.server_port)},'health':{'port':3000,'path':'/health'}};f=root/'app.json';f.write_text(json.dumps(spec))
  try:
   doctor=cli('doctor');assert doctor['hypervisor_ready'],doctor
   cli('create',str(f));created=True;release=operation('build',name);results.append('isolated Git/Bun build and OCI packaging')
   operation('deploy',name,release['id']);assert json.loads(http('/health'))['ok'];results.append('deploy and HTTP readiness')
   assert http('/host')=='defuss-physical-host';results.append('host alias reaches physical host loopback')
   http('/output');assert (root/'shares/output/hello.txt').read_text()=='hello from Bun\n';results.append('host output share')
   before=json.loads(http('/counter','POST'))['value'];backup=operation('backup',name);http('/counter','POST')
   operation('stop',name);operation('start',name);assert json.loads(http('/counter'))['value']==before+1;results.append('SQLite persistence across VM stop/start')
   operation('restore',name,backup['id']);assert json.loads(http('/counter'))['value']==before;results.append('backup, safety backup, restore')
   release2=operation('build',name);operation('deploy',name,release2['id']);assert json.loads(http('/counter'))['value']==before
   operation('rollback',name);assert json.loads(http('/counter'))['value']==before;results.append('redeploy and rollback preserve data')
   a.report.write_text(json.dumps({'passed':results,'doctor':doctor,'release':release},indent=2)+'\n');print(a.report)
  finally:
   if created:operation('delete',name,'--confirm',name)
   host.shutdown()
if __name__=='__main__':main()
