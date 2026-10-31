import { z } from 'zod';
import { Database } from 'bun:sqlite';
import { mkdirSync, writeFileSync } from 'node:fs';
mkdirSync('./var',{recursive:true});
const db=new Database('./var/counter.sqlite',{create:true});
db.exec('PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS counter(id INTEGER PRIMARY KEY, value INTEGER NOT NULL); INSERT OR IGNORE INTO counter VALUES(1,0)');
const server=Bun.serve({hostname:'0.0.0.0',port:z.coerce.number().int().min(1).max(65535).parse(process.env.PORT||3000),async fetch(request){
 const url=new URL(request.url);
 if(url.pathname==='/health')return Response.json({ok:true,runtime:Bun.version});
 if(url.pathname==='/counter'&&request.method==='POST'){db.query('UPDATE counter SET value=value+1 WHERE id=1').run();return Response.json(db.query('SELECT value FROM counter WHERE id=1').get());}
 if(url.pathname==='/counter')return Response.json(db.query('SELECT value FROM counter WHERE id=1').get());
 if(url.pathname==='/host'){const port=Number(process.env.HOST_TEST_PORT||8123);return new Response(await(await fetch(`http://host:${port}/`)).text());}
 if(url.pathname==='/output'){writeFileSync('./output/hello.txt','hello from Bun\n');return new Response('written');}
 return new Response('defuss-container example\n');
}});
console.log(`Listening on ${server.hostname}:${server.port}`);
function stop(){server.stop(true);db.close();process.exit(0);}
process.on('SIGTERM',stop);process.on('SIGINT',stop);
