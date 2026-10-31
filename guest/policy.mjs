const reserved=new Set(['var','tmp','input','output','etc']);
export function artifactExcluded(name){
 return reserved.has(name.split('/')[0])||name.split('/').some(p=>p==='.git'||p==='.npmrc'||(p.startsWith('.env')&&!p.endsWith('.example')));
}
export function sourceExcluded(name){return artifactExcluded(name)||name.split('/').includes('node_modules');}
