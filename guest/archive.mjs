import fs from 'node:fs';
import path from 'node:path';

const BLOCK = 512;
function safeName(name) {
  name = name.replace(/^\.\//, '').replace(/\/$/, '');
  if (!name || Buffer.byteLength(name)>4096 || name.includes('\0') || name.includes('\\') || path.posix.isAbsolute(name) || name.split('/').includes('..')) throw new Error(`unsafe archive path: ${name}`);
  return name;
}
function safeLink(name, target) {
  if (target.includes('\0') || target.includes('\\') || path.posix.isAbsolute(target)) throw new Error(`unsafe link: ${name}`);
  const resolved=path.posix.normalize(path.posix.join(path.posix.dirname(name),target));
  if(resolved==='..'||resolved.startsWith('../')) throw new Error(`escaping link: ${name}`);
}
function octal(b, offset, width, n) { const s=n.toString(8); if(s.length>=width) throw new Error('tar numeric overflow');b.write(s.padStart(width-1,'0')+'\0',offset,width,'ascii'); }
function header(name,size,mode,type='0',link='') {
  const b=Buffer.alloc(BLOCK);b.write(name,0,100);octal(b,100,8,mode&0o777);octal(b,108,8,0);octal(b,116,8,0);octal(b,124,12,size);octal(b,136,12,0);
  b.fill(32,148,156);b.write(type,156,1);b.write(link,157,100);b.write('ustar\0',257,6);b.write('00',263,2);
  const sum=b.reduce((s,v)=>s+v,0);b.write(sum.toString(8).padStart(6,'0')+'\0 ',148,8);return b;
}
function paxLine(k,v) {const body=` ${k}=${v}\n`;let n=Buffer.byteLength(body)+1;while(String(n).length+Buffer.byteLength(body)!==n)n=String(n).length+Buffer.byteLength(body);return `${n}${body}`;}
function writeAll(fd,data){let p=0;while(p<data.length)p+=fs.writeSync(fd,data,p,data.length-p);}
function pad(fd,n){if(n%BLOCK)writeAll(fd,Buffer.alloc(BLOCK-n%BLOCK));}
export function createTar(root,destination,{prefix='',exclude=()=>false}={}) {
  const fd=fs.openSync(destination,'wx',0o600);const buf=Buffer.alloc(1024*1024);
  try {
    function add(rel){
      if(exclude(rel))return;const source=path.join(root,rel);const st=fs.lstatSync(source);const name=safeName(prefix+rel.split(path.sep).join('/'));
      if(!st.isFile()&&!st.isDirectory()&&!st.isSymbolicLink())throw new Error(`unsupported file: ${rel}`);
      const link=st.isSymbolicLink()?fs.readlinkSync(source):'';if(link)safeLink(name,link);
      let extended='';if(Buffer.byteLength(name)>100)extended+=paxLine('path',name);if(Buffer.byteLength(link)>100)extended+=paxLine('linkpath',link);
      if(extended){const data=Buffer.from(extended);writeAll(fd,header('PaxHeader',data.length,0o644,'x'));writeAll(fd,data);pad(fd,data.length);}
      writeAll(fd,header(Buffer.byteLength(name)>100?'entry':name,st.isFile()?st.size:0,st.mode,st.isDirectory()?'5':st.isSymbolicLink()?'2':'0',Buffer.byteLength(link)>100?'':link));
      if(st.isFile()){const f=fs.openSync(source,'r');try{let n;while((n=fs.readSync(f,buf,0,buf.length,null))>0)writeAll(fd,buf.subarray(0,n));}finally{fs.closeSync(f);}pad(fd,st.size);}
      if(st.isDirectory())for(const child of fs.readdirSync(source).sort())add(path.join(rel,child));
    }
    for(const child of fs.readdirSync(root).sort())add(child);writeAll(fd,Buffer.alloc(1024));fs.fsyncSync(fd);
  } finally {fs.closeSync(fd);}
}
export function extractTar(archive,root,{maxBytes=8*1024**3,exclude=()=>false}={}) {
  fs.mkdirSync(root,{recursive:true});const realRoot=fs.realpathSync(root);const fd=fs.openSync(archive,'r');let pos=0,total=0,pax={};const seen=new Set();
  function read(n){const b=Buffer.alloc(n);let off=0;while(off<n){const count=fs.readSync(fd,b,off,n-off,pos+off);if(!count)throw new Error('truncated tar');off+=count;}pos+=n;return b;}
  const str=(b,a,n)=>b.subarray(a,a+n).toString('utf8').replace(/\0.*$/s,'');
  const num=(b,a,n)=>{const s=str(b,a,n).trim();if(!/^[0-7]*$/.test(s))throw new Error('unsupported tar number');return parseInt(s||'0',8);};
  try {while(true){const b=read(BLOCK);if(b.every(v=>v===0))break;const expected=num(b,148,8);let sum=0;for(let i=0;i<BLOCK;i++)sum+=i>=148&&i<156?32:b[i];if(sum!==expected)throw new Error('tar checksum mismatch');
      const size=num(b,124,12);total+=size;if(total>maxBytes)throw new Error('archive expanded size limit');const type=str(b,156,1)||'0';
      if(type==='x'||type==='g'){if(size>1024*1024)throw new Error('oversized PAX');const data=read(size);pos+= (BLOCK-size%BLOCK)%BLOCK;
        if(type==='x'){let at=0;while(at<data.length){const space=data.indexOf(32,at);const len=Number(data.subarray(at,space));if(!Number.isInteger(len)||len<=space-at+1||at+len>data.length)throw new Error('invalid PAX');const text=data.subarray(space+1,at+len-1).toString();const eq=text.indexOf('=');pax[text.slice(0,eq)]=text.slice(eq+1);at+=len;}}continue;}
      const raw=pax.path||[str(b,345,155),str(b,0,100)].filter(Boolean).join('/');const link=pax.linkpath||str(b,157,100);pax={};
      if(raw==='.'||raw==='./'){pos+=size+(BLOCK-size%BLOCK)%BLOCK;continue;}
      const name=safeName(raw);if(exclude(name)){pos+=size+(BLOCK-size%BLOCK)%BLOCK;continue;}
      if(!['0','2','5'].includes(type))throw new Error(`unsupported tar entry: ${type}`);
      if(seen.size>=250000)throw new Error('archive entry-count limit');
      if(seen.has(name))throw new Error(`duplicate archive entry: ${name}`);seen.add(name);
      const dest=path.join(realRoot,name);let parent=realRoot;
      for(const segment of name.split('/').slice(0,-1)){parent=path.join(parent,segment);if(fs.existsSync(parent)){if(!fs.lstatSync(parent).isDirectory()||fs.lstatSync(parent).isSymbolicLink())throw new Error('archive symlink parent');}else fs.mkdirSync(parent);}
      if(fs.existsSync(dest)||(()=>{try{return fs.lstatSync(dest).isSymbolicLink()}catch{return false}})()){if(type!=='5'||!fs.lstatSync(dest).isDirectory())throw new Error('archive destination exists');}
      if(type==='5'){fs.mkdirSync(dest,{recursive:true,mode:num(b,100,8)&0o777});if(size)throw new Error('directory with payload');}
      else if(type==='2'){safeLink(name,link);if(size)throw new Error('link with payload');fs.symlinkSync(link,dest);}
      else {const f=fs.openSync(dest,'wx',num(b,100,8)&0o777);try{let remain=size;while(remain){const chunk=read(Math.min(remain,1024*1024));writeAll(f,chunk);remain-=chunk.length;}}finally{fs.closeSync(f);}pos+=(BLOCK-size%BLOCK)%BLOCK;}
  }}finally{fs.closeSync(fd);}
}
