// Gaussian splat workers through their real message handlers, after the
// WebAssembly initialization message they expect.
import {readFileSync} from 'node:fs';
import {makeRng} from './lib.mjs';
import {invoke, loadWorker} from './workers-terrain.mjs';

const suite=module=>({module,cases:[],async add(f,a,fn){let r;try{r=await fn();}catch(e){r={throws:String(e.message??e),kind:e.name??'Error'};}this.cases.push({f,a,r});}});
const num=v=>Number.isNaN(v)?'NaN':v===Infinity?'Infinity':v===-Infinity?'-Infinity':Object.is(v,-0)?'-0':v;
const arr=v=>Array.from(v,num);
/** Texture data with its zero tail trimmed. */
const trimmed=v=>{let end=v.length;while(end>0&&v[end-1]===0)end--;return {length:v.length,values:Array.from(v.subarray(0,end))};};

export default async function workersSplats() {
  const rng=makeRng(0x5b1a7);
  const wasmBinary=readFileSync(new URL('../node_modules/@cesium/wasm-splats/wasm_splats_bg.wasm',import.meta.url));
  const sorter=await loadWorker('gaussianSplatSorter'),textures=await loadWorker('gaussianSplatTextureGenerator');
  const sort=suite('gaussianSplatSorter'),texture=suite('gaussianSplatTextureGenerator');
  // Initialization compiles the module; the Rust port has nothing to compile.
  for(const worker of [sorter,textures]) if((await invoke(worker,{webAssemblyConfig:{wasmBinary}})).result!==true) throw new Error('wasm-splats did not initialize');

  const rotation=()=>{const q=[rng()-0.5,rng()-0.5,rng()-0.5,rng()-0.5],l=Math.hypot(...q);return q.map(v=>v/l);};
  const modelView=k=>{
    if(k===0) return new Float32Array([1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]);
    const [x,y,z,w]=rotation();
    const m=[1-2*(y*y+z*z),2*(x*y+w*z),2*(x*z-w*y),0,2*(x*y-w*z),1-2*(x*x+z*z),2*(y*z+w*x),0,2*(x*z+w*y),2*(y*z-w*x),1-2*(x*x+y*y),0,rng()*10,rng()*10,-rng()*100,1];
    return new Float32Array(m);
  };
  for(let k=0;k<16;k++) {
    const count=[0,1,2,7,64,500,3000][k%7];
    const scale=[1,1e-3,50,1e5,6e5][k%5];
    const positions=new Float32Array(count*3).map(()=>k===6?Math.round(rng()*4):(rng()-0.5)*scale);
    if(k===9&&count) positions[1]=NaN;
    const mv=modelView(k%4);
    await sort.add('Index',[arr(positions),arr(mv),count],async()=>{
      const {result,transfers}=await invoke(sorter,{primitive:{positions,modelView:mv,count},sortType:'Index'});
      return {indices:Array.from(result),transferCount:transfers.length};
    });
  }
  await sort.add('Index',[[1,2,3],arr(modelView(0)),2],async()=>(await invoke(sorter,{primitive:{positions:new Float32Array([1,2,3]),modelView:modelView(0),count:2},sortType:'Index'})).result);
  await sort.add('Index',[[1,2,3],[1,0,0],1],async()=>(await invoke(sorter,{primitive:{positions:new Float32Array([1,2,3]),modelView:new Float32Array([1,0,0]),count:1},sortType:'Index'})).result);
  await sort.add('Distance',[[1,2,3],arr(modelView(0)),1],async()=>(await invoke(sorter,{primitive:{positions:new Float32Array([1,2,3]),modelView:modelView(0),count:1},sortType:'Distance'})).result??null);

  for(let k=0;k<14;k++) {
    const count=[0,1,3,40,1024,1025,200][k%7];
    const scaleRange=[1,1e-4,30,2e3,1e-8][k%5];
    const positions=new Float32Array(count*3).map(()=>(rng()-0.5)*1000);
    const scales=new Float32Array(count*3).map(()=>k===11?(rng()<0.5?0:-rng()):rng()*scaleRange);
    const rotations=new Float32Array(count*4);
    for(let i=0;i<count;i++) rotations.set(k===12?[rng()*3,rng(),-rng()*2,rng()]:rotation(),i*4);
    const colors=new Uint8Array(count*4).map(()=>Math.floor(rng()*256));
    if(k===13&&count){scales[0]=NaN;scales[4]=Infinity;}
    await texture.add('generate',[arr(positions),arr(scales),arr(rotations),Array.from(colors),count],async()=>{
      const {result,transfers}=await invoke(textures,{attributes:{positions,scales,rotations,colors},count});
      return {data:trimmed(result.data),width:result.width,height:result.height,transferCount:transfers.length};
    });
  }
  return [sort,texture];
}
