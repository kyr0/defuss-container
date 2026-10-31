// Run inside a deployed VM, against disposable subdirectories only. No cache eviction claimed.
import fs from 'node:fs';
import {performance} from 'node:perf_hooks';
import {Database} from 'bun:sqlite';
const results=[];const bytes=64*1024*1024,block=Buffer.alloc(1024*1024,0xa5);
const measured=fn=>{const start=performance.now();fn();return performance.now()-start;};
for(const base of ['/app/var','/app/output']){
 const dir=`${base}/defuss-bench-${crypto.randomUUID()}`;fs.mkdirSync(dir);const rows=[];
 try{for(let iteration=0;iteration<5;iteration++){
  const big=`${dir}/large`;let fd=fs.openSync(big,'w');const write=measured(()=>{for(let i=0;i<64;i++)fs.writeSync(fd,block);fs.fsyncSync(fd);});fs.closeSync(fd);
  fd=fs.openSync(big,'r');const read=measured(()=>{while(fs.readSync(fd,block)>0){}});fs.closeSync(fd);
  const create=measured(()=>{for(let i=0;i<1000;i++)fs.writeFileSync(`${dir}/f${i}`,block.subarray(0,4096));});
  const stat=measured(()=>{for(let i=0;i<1000;i++)fs.statSync(`${dir}/f${i}`);});
  const db=new Database(`${dir}/db.sqlite`);db.exec('PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS t(x INTEGER)');
  const transaction=measured(()=>{for(let i=0;i<100;i++){db.exec('BEGIN');db.query('INSERT INTO t VALUES(?)').run(i);db.exec('COMMIT');}});db.close();
  rows.push({iteration,write_fsync_mib_s:64/(write/1000),read_cached_mib_s:64/(read/1000),create_1000_ms:create,stat_1000_ms:stat,sqlite_100_full_commits_ms:transaction,bytes});
  for(const entry of fs.readdirSync(dir))fs.rmSync(`${dir}/${entry}`,{recursive:true,force:true});
 }results.push({path:base,samples:rows});}finally{fs.rmSync(dir,{recursive:true,force:true});}
}
console.log(JSON.stringify({runtime:Bun.version,architecture:process.arch,cache:'warm host and guest caches; no direct I/O; not raw disk throughput',results},null,2));
