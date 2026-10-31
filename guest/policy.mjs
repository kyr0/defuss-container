const reserved=new Set(['var','tmp','input','output','etc']);
export function artifactExcluded(name){
 return reserved.has(name.split('/')[0])||name.split('/').some(p=>p==='.git'||p==='.npmrc'||(p.startsWith('.env')&&!p.endsWith('.example')));
}
export function sourceExcluded(name){return artifactExcluded(name)||name.split('/').includes('node_modules');}
// VERIFIED: Bun 1.4.4 `bun run` with `--shell=bun` still requires a system /bin/sh and fails with MissingShell in
// the distroless image (probed in the guest, 2026-10-31); `bun exec` runs the same script text in Bun Shell.
// node_modules/.bin is prepended as `bun run` does; pre/post lifecycle scripts are not run.
export function packageScript(root,pkg,name,env){
 const script=pkg.scripts?.[name];
 if(typeof script!=='string'||!script.trim())throw new Error(`package.json scripts.${name} required`);
 return {cmd:[process.execPath,'exec',script],env:{...env,PATH:`${root}/node_modules/.bin:${env.PATH??''}`}};
}
