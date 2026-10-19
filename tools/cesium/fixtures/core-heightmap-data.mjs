import { GeographicTilingScheme, HeightmapTerrainData, Matrix4, TerrainProvider, TaskProcessor, WebMercatorTilingScheme } from "@cesium/engine";
import { Buffer } from "node:buffer";
import { enc, generators, makeRng } from "./lib.mjs";

// Tiny deterministic LERC inputs. Lerc1 uses one raw Float32 block; Lerc2
// uses a v2 one-sweep block. Actual decode/tessellation runs in Cesium's worker.
function lerc1(width,height,values,empty=false) {
  const out=Buffer.alloc(67+values.length*4);
  out.write("CntZImage ");out.writeInt32LE(11,10);out.writeUInt32LE(height,18);out.writeUInt32LE(width,22);out.writeDoubleLE(0.5,26);
  out.writeUInt32LE(empty?0:1,34);out.writeUInt32LE(empty?0:1,38);out.writeFloatLE(empty?0:1,46);
  out.writeUInt32LE(1,50);out.writeUInt32LE(1,54);out.writeUInt32LE(1+values.length*4,58);out.writeFloatLE(Math.max(0,...values),62);
  out[66]=empty?2:0;values.forEach((v,i)=>out.writeFloatLE(v,67+i*4));
  return Array.from(out);
}
function lerc2(width,height,values,mask) {
  const valid=mask?values.filter((_,i)=>mask[i]):values;
  let rle=Buffer.alloc(0);
  if(mask&&valid.length>0&&valid.length<values.length){
    rle=Buffer.alloc(4+Math.ceil(mask.length/8));rle.writeInt16LE(rle.length-4,0);rle.writeInt16LE(-32768,rle.length-2);
    mask.forEach((v,i)=>{if(v)rle[2+(i>>3)]|=1<<(7-(i&7));});
  }
  const out=Buffer.alloc((valid.length?63:62)+rle.length+valid.length*4);
  out.write("Lerc2 ");out.writeInt32LE(2,6);out.writeUInt32LE(height,10);out.writeUInt32LE(width,14);out.writeUInt32LE(valid.length,18);out.writeInt32LE(8,22);out.writeInt32LE(out.length,26);out.writeInt32LE(6,30);
  out.writeDoubleLE(0,34);out.writeDoubleLE(valid.length?Math.min(...valid):0,42);out.writeDoubleLE(valid.length?Math.max(...valid):0,50);out.writeUInt32LE(rle.length,58);rle.copy(out,62);out[62+rle.length]=1;
  valid.forEach((v,i)=>out.writeFloatLE(v,63+rle.length+i*4));return Array.from(out);
}
const types={Int8Array,Uint8Array,Int16Array,Uint16Array,Int32Array,Uint32Array,Float32Array,Float64Array};
const scheme=merc=>merc?new WebMercatorTilingScheme():new GeographicTilingScheme();
const structure = s => ({heightScale:s?.heightScale??1,heightOffset:s?.heightOffset??0,elementsPerHeight:s?.elementsPerHeight??1,stride:s?.stride??1,elementMultiplier:s?.elementMultiplier??256,isBigEndian:s?.isBigEndian??false,lowestEncodedHeight:s?.lowestEncodedHeight??null,highestEncodedHeight:s?.highestEncodedHeight??null});
const data=o=>new HeightmapTerrainData({buffer:o.encoding?new Uint8Array(o.buffer).buffer:new types[o.type](o.buffer),width:o.width,height:o.height,structure:o.structure?{...o.structure}:undefined,encoding:o.encoding,childTileMask:o.mask,waterMask:o.water?new Uint8Array(o.water):undefined,createdByUpsampling:o.upsampled});
const meta=d=>[d.credits??null,d.waterMask??null,d.childTileMask,d.wasCreatedByUpsampling(),structure(d._structure),d._buffer?Array.from(d._buffer):null,d._mesh?.encoding.quantization??null,d._skirtHeight??null];
const meshMeta=m=>[m.center,m.vertices,m.indices.constructor.name,m.indices,m.indexCountWithoutSkirts??null,m.vertexCountWithoutSkirts,m.minimumHeight,m.maximumHeight,m.rectangle,m.boundingSphere3D,m.occludeePointInScaledSpace??null,m.stride,m.orientedBoundingBox?[m.orientedBoundingBox.center,m.orientedBoundingBox.halfAxes]:null,[m.westIndicesSouthToNorth,m.southIndicesEastToWest,m.eastIndicesNorthToSouth,m.northIndicesWestToEast],m.encoding.quantization,m.encoding.exaggeration,m.encoding.exaggerationRelativeHeight];
const suite=module=>({module,cases:[],async add(f,args,fn){let r;try{r=enc(await fn(...args));}catch(e){r={throws:String(e.message??e),kind:e.name??"Error"};}this.cases.push({f,a:args.map(enc),r});}});

export default async function coreHeightmapData() {
  const originalSelf=globalThis.self,originalPost=globalThis.postMessage;
  globalThis.self={};
  const worker=(await import("@cesium/engine/Source/Workers/createVerticesFromHeightmap.js")).default;
  globalThis.self=originalSelf;
  const previous=TaskProcessor.prototype.scheduleTask;
  TaskProcessor.prototype.scheduleTask=async function(parameters){
    let response;globalThis.self=worker;globalThis.postMessage=r=>{response=r;};
    try{await worker.onmessage({data:{id:0,parameters,canTransferArrayBuffer:false}});if(response.error){const e=new Error(response.error.message??response.error);e.name=response.error.name??"Error";throw e;}return response.result;}
    finally{globalThis.self=originalSelf;globalThis.postMessage=originalPost;}
  };
  const s=suite("HeightmapTerrainData"),g=generators(makeRng(2000));
  try {
    for(let i=0;i<64;i++){
      const width=2+i%5,height=2+i%4,type=Object.keys(types)[i%8];
      const layout=i%4===0?{heightScale:.5,heightOffset:-100,elementsPerHeight:3,stride:4,elementMultiplier:256,isBigEndian:i%8===0,lowestEncodedHeight:10,highestEncodedHeight:100000}:i%4===1?{heightScale:2,heightOffset:30}:undefined;
      const buffer=Array.from({length:width*height*(layout?.stride??1)},()=>type.startsWith("U")?Math.floor(g.uniform(0,250)):g.uniform(-100,300));
      const o={buffer,width,height,type,structure:layout,mask:i%16,water:i%2?[0,255,32,64]:undefined,upsampled:i%5===0};
      const tile=[i%2,i%2,(i<32?3:16)+i%8],merc=i%2===0;
      const coordinates=[i<32?[-.2,-.2]:[-.1,-.1],[0,0],[1,1],[.5,.5],[.25,.75],[1.1,1.2],...(i<32?[[-2,-2]]:[]),...Array.from({length:12},()=>[g.rng(),g.rng()])];
      await s.add("lifecycle",[o,merc,tile,coordinates,i%3===0?2.5:1,40,i%13===0?0:0.25],async(o,merc,tile,coords,exaggeration,relative,quality)=>{
        const d=data(o),ts=scheme(merc),rectangle=ts.tileXYToRectangle(...tile);
        const interpolate=d=>coords.map(([x,y])=>d.interpolateHeight(rectangle,rectangle.west+(rectangle.east-rectangle.west)*x,rectangle.south+(rectangle.north-rectangle.south)*y)??null);
        const masks=Array.from({length:9},(_,j)=>d.isChildAvailable(tile[0],tile[1],tile[0]*2+(j%3),tile[1]*2+Math.floor(j/3)));
        const initial=[meta(d),interpolate(d),masks,d.upsample(ts,...tile,tile[0]*2,tile[1]*2,tile[2]+1)??null];
        const previousQuality=TerrainProvider.heightmapTerrainQuality;
        let mesh;
        try{TerrainProvider.heightmapTerrainQuality=quality;mesh=await d.createMesh({tilingScheme:ts,x:tile[0],y:tile[1],level:tile[2],exaggeration,exaggerationRelativeHeight:relative});}
        finally{TerrainProvider.heightmapTerrainQuality=previousQuality;}
        const upsampled=[];
        for(let y=0;y<2;y++)for(let x=0;x<2;x++){
          const child=await d.upsample(ts,...tile,tile[0]*2+x,tile[1]*2+y,tile[2]+1);
          const r=ts.tileXYToRectangle(tile[0]*2+x,tile[1]*2+y,tile[2]+1);
          upsampled.push([meta(child),coords.map(([x,y])=>child.interpolateHeight(r,r.west+(r.east-r.west)*x,r.south+(r.north-r.south)*y)??null)]);
        }
        return [initial,meshMeta(mesh),meta(d),interpolate(d),upsampled];
      });
    }
    // Compression: both formats, different original/decoded dimensions,
    // concatenated bands, truncation, malformed input and an all-no-data tile.
    for(let i=0;i<22;i++){
      const width=2+i%4,height=2+i%3,values=Array.from({length:width*height},(_,j)=>Math.fround((j%7)*40.25-20+i));
      let bytes=i%2?lerc1(width,height,values):lerc2(width,height,values);
      if(i===14)bytes.push(...lerc2(width,height,values.map(v=>v+1000)));
      if(i===15)bytes=bytes.slice(0,15);
      if(i===16)bytes=[1,2,3,4];
      if(i===17)bytes=lerc1(2,2,[],true);
      if(i===18)bytes=lerc2(width,height,values,values.map((_,j)=>j%2));
      if(i===19)bytes=lerc2(width,height,values,values.map(()=>0));
      if(i===20)bytes.push(...Array(20).fill(0));
      if(i===21)bytes.push(...Array(9).fill(0));
      const o={buffer:bytes,width:i===12?2:width,height:i===12?2:height,type:"Uint8Array",encoding:1};
      await s.add("lerc",[o,i%2===0],async(o,merc)=>{
        const d=data(o),ts=scheme(merc),r=ts.tileXYToRectangle(0,0,7);
        const initial=d.interpolateHeight(r,r.west,r.south)??null;
        try{const mesh=await d.createMesh({tilingScheme:ts,x:0,y:0,level:7});return [initial,meshMeta(mesh),meta(d),d.interpolateHeight(r,(r.west+r.east)/2,(r.south+r.north)/2)];}
        catch(error){return [initial,error.name,d._buffer!==undefined];}
      });
    }
    return [s];
  }finally{TaskProcessor.prototype.scheduleTask=previous;globalThis.self=originalSelf;globalThis.postMessage=originalPost;}
}
