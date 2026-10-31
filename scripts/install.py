#!/usr/bin/env python3
"""Build/install the per-user binaries, verified engine bundle, and optional login service.
Python >=3.11; Rust/Cargo, a C compiler and Git must already be installed.
Does not enable hypervisors, install drivers, or bypass OS signing policies.
"""
import argparse, hashlib, json, os, pathlib, platform, plistlib, shutil, subprocess, sys, tarfile, tempfile, urllib.request, zipfile
VERSION='1.32.4'
SHA={
 'darwin-arm64':'64bee03d87f6cbf3a1ab0e0926b99cec45fac62900b0a37da15a8271a5c6de59',
 'linux-arm64':'693b34530d9eac9d6d1b61e22b33bb34c87c511b1a8c9098fe35d9c4a77e07ce',
 'linux-x86_64':'7b0580c2ef98e183ac5c3a1567eca3a336d58aa0e2ca5904350f7d3a851205e8',
 'windows-x86_64':'97c702731ff2758b5e9f31b95acbd6aa83d534fc1cb781dd8165cd6395584ed4',
}
def default_home():
 if os.environ.get('DEFUSS_CONTAINER_HOME'):return pathlib.Path(os.environ['DEFUSS_CONTAINER_HOME']).expanduser()
 if sys.platform=='darwin':return pathlib.Path.home()/'Library/Application Support/defuss-container'
 if sys.platform=='win32':return pathlib.Path(os.environ['LOCALAPPDATA'])/'defuss-container'
 return pathlib.Path(os.environ.get('XDG_DATA_HOME',str(pathlib.Path.home()/'.local/share')))/'defuss-container'
def run(args,**kw):return subprocess.run([str(x) for x in args],check=True,**kw)
def private(path):
 path.mkdir(parents=True,exist_ok=True)
 if path.is_symlink():raise ValueError('symlink installation directory refused')
 if os.name!='nt':path.chmod(0o700)
 else:
  script="$p=$env:DEFUSS_PRIVATE_PATH;$sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User;$acl=New-Object System.Security.AccessControl.DirectorySecurity;$acl.SetOwner($sid);$acl.SetAccessRuleProtection($true,$false);foreach($s in @($sid.Value,'S-1-5-18')){$r=New-Object System.Security.AccessControl.FileSystemAccessRule((New-Object System.Security.Principal.SecurityIdentifier($s)), 'FullControl','ContainerInherit,ObjectInherit','None','Allow');$acl.AddAccessRule($r)};Set-Acl -LiteralPath $p -AclObject $acl"
  run(['powershell.exe','-NoProfile','-NonInteractive','-Command',script],env={**os.environ,'DEFUSS_PRIVATE_PATH':str(path)})
def engine_install(home,system,arch):
 key=f'{system}-{arch}'
 if key not in SHA:raise ValueError(f'Unsupported platform: {key}')
 suffix='zip' if system=='windows' else 'tar.gz';name=f'smolvm-{VERSION}-{key}.{suffix}'
 with tempfile.TemporaryDirectory(dir=home) as work:
  work=pathlib.Path(work);archive=work/name
  req=urllib.request.Request(f'https://github.com/smol-machines/smolvm/releases/download/v{VERSION}/{name}',headers={'User-Agent':'defuss-container/0.1.0'})
  h=hashlib.sha256()
  with urllib.request.urlopen(req,timeout=120) as src,archive.open('xb') as out:
   total=0
   while chunk:=src.read(1024*1024):
    total+=len(chunk)
    if total>256*1024*1024:raise ValueError('engine archive exceeds limit')
    h.update(chunk);out.write(chunk)
  if h.hexdigest()!=SHA[key]:raise ValueError('engine bundle SHA-256 mismatch')
  unpack=work/'unpack';unpack.mkdir()
  if suffix=='zip':
   with zipfile.ZipFile(archive) as z:
    for item in z.infolist():
     namepath=pathlib.PurePosixPath(item.filename)
     if namepath.is_absolute() or '..' in namepath.parts or '\\' in item.filename:raise ValueError('unsafe vendor zip path')
    z.extractall(unpack)
  else:
   # Verified upstream bundle includes absolute symlinks INSIDE its guest rootfs.
   # Extract real files first with data filtering, then links, never following a tar-created link.
   with tarfile.open(archive) as t:
    members=t.getmembers();regular=[m for m in members if not m.issym() and not m.islnk()]
    t.extractall(unpack,members=regular,filter='data')
    for m in members:
     if not(m.issym() or m.islnk()):continue
     rel=pathlib.PurePosixPath(m.name)
     if rel.is_absolute() or '..' in rel.parts:raise ValueError('unsafe vendor tar path')
     dest=unpack.joinpath(*rel.parts)
     if not dest.parent.resolve().is_relative_to(unpack.resolve()):raise ValueError('vendor link parent escapes bundle')
     if m.issym():dest.symlink_to(m.linkname)
     else:
      target=unpack/m.linkname
      if not target.resolve().is_relative_to(unpack.resolve()):raise ValueError('hardlink escapes bundle')
      os.link(target,dest)
  binary='smolvm.exe' if system=='windows' else 'smolvm'
  matches=[p for p in unpack.rglob(binary) if p.is_file() and not p.is_symlink() and 'agent-rootfs' not in p.parts]
  if len(matches)!=1:raise ValueError('engine archive layout changed')
  source=matches[0].parent;target=home/'engine'
  if target.exists():
   existing=run([target/binary,'--version'],capture_output=True,text=True).stdout.strip()
   if existing==f'smolvm {VERSION}':return target/binary
   raise ValueError('Different engine already installed; stop daemon and move engine directory before upgrading')
  shutil.move(str(source),str(target));return target/binary

def service_install(home,system):
 exe=home/'bin'/('defuss-containerd.exe' if system=='windows' else 'defuss-containerd');engine=home/'engine'/('smolvm.exe' if system=='windows' else 'smolvm');args=[str(exe),'--home',str(home),'--engine',str(engine)]
 if system=='linux':
  unit=pathlib.Path.home()/'.config/systemd/user/defuss-container.service';unit.parent.mkdir(parents=True,exist_ok=True)
  def quote(s):return '"'+s.replace('\\','\\\\').replace('"','\\"').replace('%','%%').replace('$','$$')+'"'
  unit.write_text('[Unit]\nDescription=Defuss Container daemon\nAfter=network-online.target\n\n[Service]\nType=simple\nExecStart='+ ' '.join(map(quote,args))+'\nRestart=on-failure\nRestartSec=3\nUMask=0077\n\n[Install]\nWantedBy=default.target\n')
  run(['systemctl','--user','daemon-reload']);run(['systemctl','--user','enable','--now','defuss-container.service'])
 elif system=='darwin':
  plist=pathlib.Path.home()/'Library/LaunchAgents/org.defuss.container.plist';plist.parent.mkdir(parents=True,exist_ok=True)
  data={'Label':'org.defuss.container','ProgramArguments':args,'RunAtLoad':True,'KeepAlive':True,'ProcessType':'Background','StandardOutPath':str(home/'daemon.stdout.log'),'StandardErrorPath':str(home/'daemon.stderr.log'),'EnvironmentVariables':{'PATH':'/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin'},'Umask':63}
  plist.write_bytes(plistlib.dumps(data));plist.chmod(0o600);domain=f'gui/{os.getuid()}'
  subprocess.run(['launchctl','bootout',f'{domain}/org.defuss.container'],capture_output=True)
  run(['launchctl','bootstrap',domain,str(plist)]);run(['launchctl','kickstart',f'{domain}/org.defuss.container'])
 else:
  ps=home/'register-service.ps1';ps.write_text("$ErrorActionPreference='Stop'\n$config=Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'service.json')|ConvertFrom-Json\n$action=New-ScheduledTaskAction -Execute $config.exe -Argument $config.args\n$trigger=New-ScheduledTaskTrigger -AtLogOn -User ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name)\n$principal=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited\n$settings=New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew\nRegister-ScheduledTask -TaskName 'Defuss Container' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null\nStart-ScheduledTask -TaskName 'Defuss Container'\n")
  (home/'service.json').write_text(json.dumps({'exe':str(exe),'args':subprocess.list2cmdline(args[1:])}))
  run(['powershell.exe','-NoProfile','-NonInteractive','-File',ps])

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--home',type=pathlib.Path,default=default_home());parser.add_argument('--service',action='store_true',help='install/start per-user login service');parser.add_argument('--engine-only',action='store_true');a=parser.parse_args();home=a.home.expanduser().absolute();private(home)
 system={'darwin':'darwin','win32':'windows'}.get(sys.platform,'linux');arch={'aarch64':'arm64','arm64':'arm64','AMD64':'x86_64','x86_64':'x86_64'}.get(platform.machine(),platform.machine())
 engine=engine_install(home,system,arch)
 if not a.engine_only:
  root=pathlib.Path(__file__).resolve().parents[1];run(['cargo','build','--release','--locked','--manifest-path',root/'Cargo.toml'],cwd=root);private(home/'bin')
  # Ask Cargo: CARGO_TARGET_DIR or build.target-dir can move binaries out of ./target.
  target=pathlib.Path(json.loads(run(['cargo','metadata','--format-version','1','--no-deps','--manifest-path',root/'Cargo.toml'],cwd=root,capture_output=True).stdout)['target_directory'])/'release'
  for name in ['defuss-container','defuss-containerd']:
   name+= '.exe' if system=='windows' else '';shutil.copy2(target/name,home/'bin'/name)
  if a.service:service_install(home,system)
 print(json.dumps({'home':str(home),'engine':str(engine),'service_installed':a.service and not a.engine_only,'bin':str(home/'bin')},indent=2))
if __name__=='__main__':main()
