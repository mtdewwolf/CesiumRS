// Vector tile workers through their real message handlers: points, polylines,
// clamped polylines, polygons and instanced geometries.
import * as C from '@cesium/engine';
import {makeRng} from './lib.mjs';
import {invoke, loadWorker} from './workers-terrain.mjs';

const suite=module=>({module,cases:[],async add(f,a,fn){let r;try{r=await fn();}catch(e){r={throws:String(e.message??e),kind:e.name??'Error'};}this.cases.push({f,a,r});}});
// Plain JSON with special numbers spelled out, as lib.mjs `enc` does for scalars.
const num=v=>Number.isNaN(v)?'NaN':v===Infinity?'Infinity':v===-Infinity?'-Infinity':Object.is(v,-0)?'-0':v;
const arr=v=>Array.from(v,num);
/** Large outputs are compared through their length, ends and two sums. */
export const digest=v=>{
  const a=Array.from(v);let sum=0,weighted=0;
  for(let i=0;i<a.length;i++){sum+=a[i];weighted+=a[i]*((i%7)+1);}
  return {length:a.length,head:arr(a.slice(0,24)),tail:arr(a.slice(-24)),sum:num(sum),weighted:num(weighted)};
};

function zigZag(values) {
  const out=new Uint16Array(values.length);let previous=0;
  for(let i=0;i<values.length;i++){const d=values[i]-previous;previous=values[i];out[i]=((d<<1)^(d>>31))&0xffff;}
  return out;
}
function encode(us,vs,hs) {
  const planes=[zigZag(us),zigZag(vs)];if(hs) planes.push(zigZag(hs));
  const out=new Uint16Array(planes.reduce((n,p)=>n+p.length,0));let o=0;
  for(const p of planes){out.set(p,o);o+=p.length;}
  return out;
}
const ellipsoids=[C.Ellipsoid.WGS84,C.Ellipsoid.UNIT_SPHERE.radii.x?new C.Ellipsoid(6378137,6378137,6356752.3142451793):C.Ellipsoid.WGS84,new C.Ellipsoid(1737400,1737400,1737400)];
function tilePacked(kind,rectangle,ellipsoid,min,max,center) {
  const r=[rectangle.west,rectangle.south,rectangle.east,rectangle.north],e=[ellipsoid.radii.x,ellipsoid.radii.y,ellipsoid.radii.z];
  if(kind==='points') return new Float64Array([min,max,...r,...e]);
  if(kind==='polylines') return new Float64Array([min,max,...r,...e,center.x,center.y,center.z]);
  return null;
}
const buffer=t=>t.slice().buffer;

export default async function workersVector() {
  const rng=makeRng(0x5ec7);
  const int=(lo,hi)=>lo+Math.floor(rng()*(hi-lo+1));
  const points=suite('createVectorTilePoints'),polylines=suite('createVectorTilePolylines'),clamped=suite('createVectorTileClampedPolylines');
  const polygons=suite('createVectorTilePolygons'),geometries=suite('createVectorTileGeometries');
  const workers={};
  for(const name of ['createVectorTilePoints','createVectorTilePolylines','createVectorTileClampedPolylines','createVectorTilePolygons','createVectorTileGeometries'])
    workers[name]=await loadWorker(name);
  const rectangles=[C.Rectangle.fromDegrees(-75.6,40.0,-75.5,40.1),C.Rectangle.fromDegrees(170,-10,-170,5),C.Rectangle.fromDegrees(-1,-1,1,1),C.Rectangle.fromDegrees(10,80,40,89.9)];

  // Points.
  for(let k=0;k<12;k++) {
    const n=[0,1,2,7,40][k%5],rectangle=rectangles[k%4],ellipsoid=ellipsoids[k%3];
    const us=Array.from({length:n},()=>k===11?int(0,1)*32767:int(0,32767)),vs=Array.from({length:n},()=>int(0,32767)),hs=Array.from({length:n},()=>int(0,32767));
    const positions=encode(us,vs,hs),packed=tilePacked('points',rectangle,ellipsoid,k%2?-100:0,k%3?5000:10);
    await points.add('create',[arr(positions),arr(packed)],async()=>{
      const {result,transfers}=await invoke(workers.createVectorTilePoints,{positions:buffer(positions),packedBuffer:buffer(packed)});
      return {positions:arr(new Float64Array(result.positions)),transferCount:transfers.length};
    });
  }
  await points.add('create',[[1,2,3,4],arr(tilePacked('points',rectangles[0],ellipsoids[0],0,1))],async()=>{
    const {result}=await invoke(workers.createVectorTilePoints,{positions:new Uint16Array([1,2,3,4]).buffer,packedBuffer:buffer(tilePacked('points',rectangles[0],ellipsoids[0],0,1))});
    return {positions:arr(new Float64Array(result.positions)),transferCount:1};
  });

  // Polylines and clamped polylines share inputs.
  const polylineInputs=[];
  for(let k=0;k<14;k++) {
    const counts=k===0?[2]:k===1?[3,2,5]:k===2?[1,4]:k===3?[0,3]:Array.from({length:int(1,6)},()=>int(2,9));
    const n=counts.reduce((a,b)=>a+b,0)+(k===4?2:0);
    const us=[],vs=[],hs=[];
    let p=0;
    for(const count of counts) {
      let u=int(2000,30000),v=int(2000,30000);
      const loop=k%5===1&&count>2,start=[u,v];
      for(let j=0;j<count;j++) {
        if(loop&&j===count-1) [u,v]=start;
        else if(j>0) {
          // Duplicates, sharp reversals and ordinary steps.
          const mode=k>=6?int(0,5):0;
          if(mode===0&&k%3===0&&j%3===2) {}
          else if(mode===1) {u-=int(300,900);v+=int(-30,30);}
          else {u+=int(-900,900);v+=int(-900,900);}
        }
        us.push(Math.max(0,Math.min(32767,u)));vs.push(Math.max(0,Math.min(32767,v)));hs.push(int(0,32767));p++;
      }
    }
    while(us.length<n){us.push(int(0,32767));vs.push(int(0,32767));hs.push(int(0,32767));}
    const rectangle=rectangles[k%4],ellipsoid=ellipsoids[k%2];
    const center=ellipsoid.cartographicToCartesian(C.Rectangle.center(rectangle));
    polylineInputs.push({positions:encode(us,vs,hs),widths:Uint16Array.from(counts,()=>int(1,20)),counts:Uint32Array.from(counts),
      batchIds:Uint16Array.from(counts,(_,i)=>(i*7+k)%65536),packed:tilePacked('polylines',rectangle,ellipsoid,k%2?-50:0,k%3?800:30,center),keep:k%2===0});
  }
  // A wide polyline crossing the 65536-vertex index boundary.
  {
    const n=16390,us=Array.from({length:n},(_,i)=>(i*2)%32768),vs=Array.from({length:n},(_,i)=>Math.floor(i/16)%32768),hs=Array.from({length:n},(_,i)=>i%1000);
    const rectangle=rectangles[0],center=C.Ellipsoid.WGS84.cartographicToCartesian(C.Rectangle.center(rectangle));
    polylineInputs.push({positions:encode(us,vs,hs),widths:new Uint16Array([3]),counts:new Uint32Array([n]),batchIds:new Uint16Array([9]),
      packed:tilePacked('polylines',rectangle,C.Ellipsoid.WGS84,0,100,center),keep:false,large:true});
  }
  const outputs=(result,names,large)=>Object.fromEntries(names.map(([name,Type])=>{const v=new Type(result[name]);return [name,large||v.length>3000?digest(v):arr(v)];}));
  for(const input of polylineInputs) {
    const args=[arr(input.positions),arr(input.widths),arr(input.counts),arr(input.batchIds),arr(input.packed),input.keep];
    const parameters=()=>({positions:buffer(input.positions),widths:buffer(input.widths),counts:buffer(input.counts),batchIds:buffer(input.batchIds),
      packedBuffer:buffer(input.packed),keepDecodedPositions:input.keep});
    await polylines.add('create',args,async()=>{
      const {result,transfers}=await invoke(workers.createVectorTilePolylines,parameters());
      const IndexType=result.indexDatatype===C.IndexDatatype.UNSIGNED_INT?Uint32Array:Uint16Array;
      return {indexDatatype:result.indexDatatype,...outputs(result,[['currentPositions',Float32Array],['previousPositions',Float32Array],['nextPositions',Float32Array],
        ['expandAndWidth',Float32Array],['batchIds',Uint16Array],['indices',IndexType]],input.large),
        decodedPositions:result.decodedPositions?arr(new Float64Array(result.decodedPositions)):null,
        decodedPositionOffsets:result.decodedPositionOffsets?arr(new Uint32Array(result.decodedPositionOffsets)):null,transferCount:transfers.length};
    });
    if(input.large) continue;
    await clamped.add('create',args,async()=>{
      const {result,transfers}=await invoke(workers.createVectorTileClampedPolylines,parameters());
      const IndexType=result.indexDatatype===C.IndexDatatype.UNSIGNED_INT?Uint32Array:Uint16Array;
      return {indexDatatype:result.indexDatatype,...outputs(result,[['startEllipsoidNormals',Float32Array],['endEllipsoidNormals',Float32Array],
        ['startPositionAndHeights',Float32Array],['startFaceNormalAndVertexCornerIds',Float32Array],['endPositionAndHeights',Float32Array],
        ['endFaceNormalAndHalfWidths',Float32Array],['vertexBatchIds',Uint16Array],['indices',IndexType]],false),
        decodedPositions:result.decodedPositions?arr(new Float64Array(result.decodedPositions)):null,
        decodedPositionOffsets:result.decodedPositionOffsets?arr(new Uint32Array(result.decodedPositionOffsets)):null,transferCount:transfers.length};
    });
  }
  // Clamped polylines past 8192 volumes switch to 32-bit indices.
  {
    const n=8200,us=Array.from({length:n},(_,i)=>1000+(i*3)%30000),vs=Array.from({length:n},(_,i)=>1000+Math.floor(i/10000)*50+(i%2)*7),hs=Array.from({length:n},(_,i)=>i%500);
    const rectangle=rectangles[0],center=C.Ellipsoid.WGS84.cartographicToCartesian(C.Rectangle.center(rectangle));
    const input={positions:encode(us,vs,hs),widths:new Uint16Array([4]),counts:new Uint32Array([n]),batchIds:new Uint16Array([2]),packed:tilePacked('polylines',rectangle,C.Ellipsoid.WGS84,0,100,center)};
    await clamped.add('create',[arr(input.positions),arr(input.widths),arr(input.counts),arr(input.batchIds),arr(input.packed),false],async()=>{
      const {result,transfers}=await invoke(workers.createVectorTileClampedPolylines,{positions:buffer(input.positions),widths:buffer(input.widths),counts:buffer(input.counts),
        batchIds:buffer(input.batchIds),packedBuffer:buffer(input.packed),keepDecodedPositions:false});
      const IndexType=result.indexDatatype===C.IndexDatatype.UNSIGNED_INT?Uint32Array:Uint16Array;
      return {indexDatatype:result.indexDatatype,...outputs(result,[['startEllipsoidNormals',Float32Array],['endEllipsoidNormals',Float32Array],
        ['startPositionAndHeights',Float32Array],['startFaceNormalAndVertexCornerIds',Float32Array],['endPositionAndHeights',Float32Array],
        ['endFaceNormalAndHalfWidths',Float32Array],['vertexBatchIds',Uint16Array],['indices',IndexType]],true),decodedPositions:null,decodedPositionOffsets:null,transferCount:transfers.length};
    });
  }

  // Polygons: convex rings triangulated as fans, batched by repeated colors.
  const polygonCase=async(k,{large=false}={})=>{
    const polygonCount=large?3:[1,2,4,6][k%4];
    const counts=[],indexCounts=[],us=[],vs=[],indices=[],colors=[],batchIds=[];
    const palette=[0xff0000ff,0x00ff00ff,0xffffffff,0x0a0b0c0d,0,0xffffffff];
    let offset=0;
    for(let i=0;i<polygonCount;i++) {
      const count=large?11000:int(3,9),cu=int(5000,27000),cv=int(5000,27000),radius=int(200,3000);
      for(let j=0;j<count;j++){const a=2*Math.PI*j/count;us.push(Math.round(cu+radius*Math.cos(a)));vs.push(Math.round(cv+radius*Math.sin(a)));}
      for(let j=1;j<count-1;j++) indices.push(offset,offset+j,offset+j+1);
      counts.push(count);indexCounts.push(3*(count-2));offset+=count;
      colors.push(palette[(i+k)%palette.length]);batchIds.push(large?i:(i*3+k)%70000);
    }
    const bytes=large||k%3===2?4:2;
    const rectangle=rectangles[k%3],ellipsoid=ellipsoids[k%2];
    const center=ellipsoid.cartographicToCartesian(C.Rectangle.center(rectangle));
    const packed=new Float64Array([bytes,k%2?-20:0,k%3?300:50,center.x,center.y,center.z,ellipsoid.radii.x,ellipsoid.radii.y,ellipsoid.radii.z,rectangle.west,rectangle.south,rectangle.east,rectangle.north]);
    const perPolygon=k%4===1;
    const minimumHeights=perPolygon?Float32Array.from(counts,(_,i)=>-10*i):undefined,maximumHeights=perPolygon?Float32Array.from(counts,(_,i)=>100+i):undefined;
    const input={packed,indices:bytes===2?new Uint16Array(indices):new Uint32Array(indices),positions:encode(us,vs),counts:Uint32Array.from(counts),
      indexCounts:Uint32Array.from(indexCounts),batchIds:Uint32Array.from(batchIds),colors:Uint32Array.from(colors)};
    await polygons.add('create',[arr(input.packed),arr(input.indices),arr(input.positions),arr(input.counts),arr(input.indexCounts),arr(input.batchIds),arr(input.colors),
      minimumHeights?arr(minimumHeights):null,maximumHeights?arr(maximumHeights):null],async()=>{
      const {result,transfers}=await invoke(workers.createVectorTilePolygons,{packedBuffer:buffer(input.packed),indices:buffer(input.indices),positions:buffer(input.positions),
        counts:buffer(input.counts),indexCounts:buffer(input.indexCounts),batchIds:buffer(input.batchIds),batchTableColors:buffer(input.colors),
        minimumHeights:minimumHeights?buffer(minimumHeights):undefined,maximumHeights:maximumHeights?buffer(maximumHeights):undefined});
      const packedOut=new Float64Array(result.packedBuffer);
      const IndexType=packedOut[0]===C.IndexDatatype.UNSIGNED_INT?Uint32Array:Uint16Array;
      return {...outputs(result,[['positions',Float32Array],['indices',IndexType],['indexOffsets',Uint32Array],['indexCounts',Uint32Array],['batchIds',Uint16Array]],large),
        packedBuffer:arr(packedOut),transferCount:transfers.length};
    });
  };
  for(let k=0;k<12;k++) await polygonCase(k);
  await polygonCase(0,{large:true});

  // Instanced geometries.
  const geometryCase=async(k,{large=false}={})=>{
    const kinds=large?{ellipsoids:32}:k<3?{boxes:int(1,4)}:{boxes:k%5===4?0:int(1,3),cylinders:k%3===0?0:int(1,2),ellipsoids:k%4===1?0:int(1,2),spheres:k%2?int(1,3):0};
    const randomMatrix=()=>Array.from(C.Matrix4.fromTranslationQuaternionRotationScale(new C.Cartesian3(int(-50,50),int(-50,50),int(-50,50)),
      C.Quaternion.fromAxisAngle(C.Cartesian3.normalize(new C.Cartesian3(rng()+0.1,rng(),rng()),new C.Cartesian3()),rng()*6),new C.Cartesian3(1,1,1)));
    const parameters={},args={};
    let id=0;
    for(const [name,count] of Object.entries(kinds)) {
      if(!count) continue;
      const values=[];
      for(let i=0;i<count;i++) {
        if(name==='boxes'||name==='ellipsoids') values.push(int(1,9),int(1,9),int(1,9),...randomMatrix());
        else if(name==='cylinders') values.push(int(1,9),int(2,20),...randomMatrix());
        else values.push(int(1,9),int(-30,30),int(-30,30),int(-30,30));
      }
      const singular=name.slice(0,-1).replace(/e$/,'')==='box'?'box':name==='ellipsoids'?'ellipsoid':name.slice(0,-1);
      parameters[name]=new Float32Array(values);parameters[singular+'BatchIds']=Uint16Array.from({length:count},()=>id++%5);
      args[name]=arr(parameters[name]);args[singular+'BatchIds']=arr(parameters[singular+'BatchIds']);
    }
    const colors=new Uint32Array([0xff0000ff,0x00ff00ff,0x0000ffff,0x80808080,0xffffffff]);
    const center=new C.Cartesian3(int(-100,100),int(-100,100),int(-100,100));
    const packed=new Float64Array([center.x,center.y,center.z,...Array.from(k%2?C.Matrix4.IDENTITY:C.Matrix4.fromTranslation(new C.Cartesian3(1e6,2e6,3e6)))]);
    await geometries.add('create',[args,arr(colors),arr(packed)],async()=>{
      const message={batchTableColors:buffer(colors),packedBuffer:buffer(packed)};
      for(const [name,value] of Object.entries(parameters)) message[name]=buffer(value);
      const {result,transfers}=await invoke(workers.createVectorTileGeometries,message);
      const packedOut=new Float64Array(result.packedBuffer);
      const IndexType=packedOut[0]===4?Uint32Array:Uint16Array;
      return {...outputs(result,[['positions',Float32Array],['vertexBatchIds',Uint16Array],['indices',IndexType],['indexOffsets',Uint32Array],['indexCounts',Uint32Array],['batchIds',Uint16Array]],large),
        packedBuffer:arr(packedOut),transferCount:transfers.length};
    });
  };
  for(let k=0;k<12;k++) await geometryCase(k);
  await geometryCase(0,{large:true});
  // Batch ids without geometry values are ignored; values without batch ids throw.
  await geometries.add('create',[{boxBatchIds:[1]},[255],arr(new Float64Array(19))],async()=>{
    const {result}=await invoke(workers.createVectorTileGeometries,{boxBatchIds:new Uint16Array([1]).buffer,batchTableColors:new Uint32Array([255]).buffer,packedBuffer:new Float64Array(19).buffer});
    return {positions:arr(new Float32Array(result.positions)),vertexBatchIds:[],indices:[],indexOffsets:[],indexCounts:[],batchIds:[],packedBuffer:arr(new Float64Array(result.packedBuffer)),transferCount:7};
  });
  await geometries.add('create',[{spheres:[1,2,3,4]},[255],arr(new Float64Array(19))],async()=>{
    return (await invoke(workers.createVectorTileGeometries,{spheres:new Float32Array([1,2,3,4]).buffer,batchTableColors:new Uint32Array([255]).buffer,packedBuffer:new Float64Array(19).buffer})).result;
  });
  return [points,polylines,clamped,polygons,geometries];
}
