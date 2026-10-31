import fs from 'node:fs';
import path from 'node:path';
import {extractTar,createTar} from './archive.mjs';
import {sourceExcluded,artifactExcluded} from './policy.mjs';
const [release, doBuild] = process.argv.slice(2);
if(!/^[0-9a-f-]{36}$/.test(release))throw new Error('invalid release id');
const root=process.argv[4]||'/tmp/defuss-build';
const source=process.argv[5]||'/source/source.tar';
const output=process.argv[6]||'/export/app.tar';fs.mkdirSync(root,{recursive:true});
extractTar(source,root,{exclude:sourceExcluded});
const pkg=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
for(const k of ['start','stop'])if(typeof pkg.scripts?.[k]!=='string'||!pkg.scripts[k].trim())throw new Error(`package.json scripts.${k} required`);
if(!fs.existsSync(path.join(root,'bun.lock'))&&!fs.existsSync(path.join(root,'bun.lockb')))throw new Error('commit bun.lock or bun.lockb before building');
function run(args){const p=Bun.spawnSync([process.execPath,...args],{cwd:root,stdout:'inherit',stderr:'inherit',env:{...process.env,BUN_CONFIG_NO_CLEAR_TERMINAL:'1'}});if(p.exitCode!==0)throw new Error(`bun ${args[0]} exited ${p.exitCode}`);}
run(['install','--frozen-lockfile','--ignore-scripts']);
if(doBuild==='true'){if(!pkg.scripts?.build)throw new Error('build requested but scripts.build absent');run(['run','--shell','bun','build']);}
// Prune development dependencies by reinstalling from the committed lockfile.
fs.rmSync(path.join(root,'node_modules'),{recursive:true,force:true});
run(['install','--production','--frozen-lockfile','--ignore-scripts']);
createTar(root,output,{prefix:`opt/defuss/releases/${release}/`,exclude:artifactExcluded});
console.log(JSON.stringify({release,bytes:fs.statSync(output).size}));
