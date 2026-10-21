// Terrain and packet workers through their real createTaskProcessorWorker
// message handlers: 3D Tiles terrain creation/upsampling, Google Earth
// Enterprise terrain buffers and encrypted packets.
import * as C from '@cesium/engine';
import {MeshoptEncoder} from 'meshoptimizer';
import Axis from '@cesium/engine/Source/Scene/Axis.js';
import {deflateSync, gzipSync} from 'node:zlib';
import {enc, makeRng} from './lib.mjs';

const suite=module=>({module,cases:[],async add(f,args,fn){let r;try{r=enc(await fn(...args));}catch(e){r={throws:String(e.message??e),kind:e.name??'Error'};}this.cases.push({f,a:args.map(enc),r});}});

export async function loadWorker(name) {
  const previous=globalThis.self;
  globalThis.self={};
  try { return (await import(`@cesium/engine/Source/Workers/${name}.js`)).default; }
  finally { if(previous===undefined) delete globalThis.self; else globalThis.self=previous; }
}

/** Posts one task to a worker's onmessage handler, as TaskProcessor does. */
export async function invoke(worker,parameters,canTransferArrayBuffer=true) {
  let response,transfers;
  const previousSelf=globalThis.self,previousPost=globalThis.postMessage;
  globalThis.self=worker;
  globalThis.postMessage=(r,t=[])=>{response=r;transfers=t;};
  try { await worker.onmessage({data:{id:7,parameters,canTransferArrayBuffer,baseUrl:'https://cesium.test/'}}); }
  finally {
    if(previousSelf===undefined) delete globalThis.self; else globalThis.self=previousSelf;
    if(previousPost===undefined) delete globalThis.postMessage; else globalThis.postMessage=previousPost;
  }
  if(response.id!==7) throw new Error('Worker response lost its id');
  if(response.error!==undefined) {
    const e=response.error;
    if(process.env.WORKER_STACK) console.error(e.stack);
    throw Object.assign(new Error(e.message??String(e)),{name:e.name??'Error'});
  }
  return {result:response.result,transfers};
}

export const encoding=e=>({quantization:e.quantization,minimumHeight:e.minimumHeight,maximumHeight:e.maximumHeight,
  center:e.center,toScaledENU:e.toScaledENU,fromScaledENU:e.fromScaledENU,matrix:e.matrix,
  hasVertexNormals:e.hasVertexNormals,hasWebMercatorT:e.hasWebMercatorT,hasGeodeticSurfaceNormals:e.hasGeodeticSurfaceNormals,
  exaggeration:e.exaggeration,exaggerationRelativeHeight:e.exaggerationRelativeHeight,stride:e.stride});
const obb=b=>b===undefined?null:[b.center,b.halfAxes];

// ---- 3D Tiles terrain ------------------------------------------------------

function gridGltf(rng,rectangle,size,normals,compression) {
  const metersEast=rectangle.width*6378137*Math.cos(rectangle.south),metersNorth=rectangle.height*6378137;
  const corner=C.Cartesian3.fromRadians(rectangle.west,rectangle.south,0);
  const enu=C.Transforms.eastNorthUpToFixedFrame(corner);
  const node=C.Matrix4.multiply(Axis.Z_UP_TO_Y_UP,C.Matrix4.multiplyByScale(enu,new C.Cartesian3(metersEast,metersNorth,400),new C.Matrix4()),new C.Matrix4());
  const json={buffers:[],bufferViews:[],accessors:[],nodes:[{matrix:Array.from(node)}],meshes:[{primitives:[{attributes:{},extensions:{CESIUM_tile_edges:{}}}]}]},buffers=[];
  const p=json.meshes[0].primitives[0];
  const add=(values,kind,name)=>{
    const components=kind==='position'||kind==='normal'?3:1,count=values.length/components;
    let bytes,stride=components===3?12:2;
    if(compression&&kind==='position') {
      // Meshopt positions are KHR_mesh_quantization Uint16 x4.
      stride=8;bytes=new Uint8Array(Uint16Array.from({length:count*4},(_,i)=>i%4===3?0:Math.round(values[(i>>2)*3+i%4]*65535)).buffer);
    } else if(compression&&kind==='normal') {
      // Meshopt normals are Int8 octahedral x4.
      stride=4;bytes=new Uint8Array(Int8Array.from({length:count*4},(_,i)=>{
        const v=values.slice((i>>2)*3,(i>>2)*3+3),l=Math.abs(v[0])+Math.abs(v[1])+Math.abs(v[2]);
        return i%4<2?Math.round(v[i%4]/l*127):0;}).buffer);
    } else bytes=new Uint8Array((components===3?new Float32Array(values):new Uint16Array(values)).buffer);
    const mode=kind==='index'?'TRIANGLES':kind==='edge'?'INDICES':'ATTRIBUTES';
    // Meshopt index/edge streams need a 2 or 4 byte stride; attributes need a multiple of 4.
    const stored=compression?MeshoptEncoder.encodeGltfBuffer(bytes,count,stride,mode):bytes;
    const view={buffer:buffers.length,byteOffset:0,byteLength:bytes.length,...(compression&&stride>2?{byteStride:stride}:{})};
    if(compression) view.extensions={EXT_meshopt_compression:{buffer:buffers.length,byteOffset:0,byteLength:stored.length,byteStride:stride,count,mode}};
    const ai=json.accessors.length;
    buffers.push(Array.from(stored));
    json.bufferViews.push(view);
    json.accessors.push({bufferView:json.bufferViews.length-1,byteOffset:0,componentType:components===3?(compression?(kind==='normal'?5120:5123):5126):5123,
      normalized:compression&&components===3?true:undefined,count,type:components===3?'VEC3':'SCALAR'});
    if(kind==='position') p.attributes.POSITION=ai; else if(kind==='normal') p.attributes.NORMAL=ai;
    else if(kind==='index') p.indices=ai; else p.extensions.CESIUM_tile_edges[name]=ai;
  };
  const positions=[],normal=[],indices=[],left=[],bottom=[],right=[],top=[];
  for(let y=0;y<size;y++) for(let x=0;x<size;x++) {
    const i=y*size+x;
    positions.push(x/(size-1),y/(size-1),rng());
    const n=new C.Cartesian3(rng()-0.5,rng()-0.5,1);C.Cartesian3.normalize(n,n);normal.push(n.x,n.y,n.z);
    if(x===0) left.push(i); if(x===size-1) right.push(i); if(y===0) bottom.push(i); if(y===size-1) top.push(i);
    if(x<size-1&&y<size-1) {
      const flip=rng()<0.5;
      if(flip) indices.push(i,i+1,i+size+1,i,i+size+1,i+size);
      else indices.push(i,i+1,i+size,i+1,i+size+1,i+size);
    }
  }
  add(positions,'position');
  if(normals) add(normal,'normal');
  add(indices,'index');
  for(const [name,values] of [['left',left],['bottom',bottom],['right',right],['top',top]]) add(values,'edge',name);
  if(compression) json.extensionsRequired=['EXT_meshopt_compression'];
  return {json,buffers};
}
function bufferGltf(g) {
  const copy=structuredClone(g.json);
  copy.buffers=g.buffers.map(b=>({byteLength:b.length,extras:{_pipeline:{source:Uint8Array.from(b)}}}));
  return copy;
}
const sized=(count,buffer)=>Array.from(count<=65535?new Uint16Array(buffer):new Uint32Array(buffer));
function meshProxy(r) {
  const stride=r.encoding.stride,vertexCount=r.verticesBuffer.byteLength/4/stride;
  return {vertices:Array.from(new Float32Array(r.verticesBuffer)),indexBytes:r.indicesBuffer.byteLength,
    indices:sized(vertexCount,r.indicesBuffer),vertexCountWithoutSkirts:r.vertexCountWithoutSkirts,
    indexCountWithoutSkirts:r.indexCountWithoutSkirts,encoding:encoding(r.encoding),
    edges:[r.westIndicesBuffer,r.southIndicesBuffer,r.eastIndicesBuffer,r.northIndicesBuffer].map(b=>sized(vertexCount,b)),
    minimumHeight:r.minimumHeight,maximumHeight:r.maximumHeight,boundingSphere:r.boundingSphere,
    orientedBoundingBox:obb(r.orientedBoundingBox),horizonOcclusionPoint:r.horizonOcclusionPoint};
}

// ---- Google Earth Enterprise ------------------------------------------------

/** Four quads of size×size grid points; shared borders repeat the same heights. */
function geeTerrain(rng,rect,size,{negative=false,flat=false}={}) {
  const height=(lon,lat)=>flat?0:Math.sin(lon*977)*80+Math.cos(lat*613)*45+((lon*1e7|0)%7);
  const parts=[];
  for(const [dx,dy] of [[0,0],[1,0],[0,1],[1,1]]) {
    const originX=rect.west+rect.width/2*dx,originY=rect.south+rect.height/2*dy;
    const stepX=rect.width/2/(size-1),stepY=rect.height/2/(size-1);
    const points=size*size,faces=2*(size-1)*(size-1);
    const b=Buffer.alloc(44+points*6+faces*6);let p=0;
    for(const v of [originX/Math.PI,originY/Math.PI,stepX/Math.PI,stepY/Math.PI]) {b.writeDoubleLE(v,p);p+=8;}
    for(const v of [points,faces,dx+dy]) {b.writeInt32LE(v,p);p+=4;}
    for(let y=0;y<size;y++) for(let x=0;x<size;x++) {
      b[p++]=x;b[p++]=y;
      // Recompute coordinates as the worker does so shared borders match exactly.
      const lon=C.Math.toRadians((originX/Math.PI)*180)+x*C.Math.toRadians((stepX/Math.PI)*180);
      const lat=C.Math.toRadians((originY/Math.PI)*180)+y*C.Math.toRadians((stepY/Math.PI)*180);
      let h=height(lon,lat)/6371010;
      if(negative&&(x+y)%3===0) h=-Math.abs(h)/2**32;
      b.writeFloatLE(h,p);p+=4;
    }
    for(let y=0;y<size-1;y++) for(let x=0;x<size-1;x++) {
      const i=y*size+x;
      for(const v of (rng()<0.5?[i,i+1,i+size,i+1,i+size+1,i+size]:[i,i+1,i+size+1,i,i+size+1,i+size])) {b.writeUInt16LE(v,p);p+=2;}
    }
    const header=Buffer.alloc(4);header.writeUInt32LE(b.length);
    parts.push(header,b);
  }
  return Buffer.concat(parts);
}

function metadataPacket(bits,{magic=32301,type=1,version=2,instanceSize=32,offsetFix=0,sizeFix=0}={}) {
  const n=bits.length,b=Buffer.alloc(32+n*32);
  [magic,type,version,n,instanceSize,32+n*32+offsetFix,sizeFix,0].forEach((v,i)=>b.writeUInt32LE(v>>>0,i*4));
  bits.forEach((v,i)=>{const o=32+i*32;b[o]=v;b.writeUInt16LE(5+i,o+2);b.writeUInt16LE(6+i,o+4);b.writeUInt16LE(7+i,o+6);b[o+28]=2+i%3;b[o+29]=3+i%4;});
  return b;
}
function terrainPacket(rng,sizes,trailing=0) {
  const parts=[];
  for(const size of sizes) {
    const header=Buffer.alloc(4);header.writeUInt32LE(size);
    parts.push(header,Buffer.from(Array.from({length:size},()=>Math.floor(rng()*256))));
  }
  parts.push(Buffer.alloc(trailing,7));
  return Buffer.concat(parts);
}
function compressed(bytes,{swap=false,gzip=false,sizeFix=0}={}) {
  const header=Buffer.alloc(8);
  if(swap) {header.writeUInt32LE(0xadde6874);header.writeUInt32BE(bytes.length+sizeFix,4);}
  else {header.writeUInt32LE(0x7468dead);header.writeUInt32LE(bytes.length+sizeFix,4);}
  return Buffer.concat([header,gzip?gzipSync(bytes):deflateSync(bytes)]);
}
/** Obfuscates a packet so decodeGoogleEarthEnterpriseData restores it. */
function encrypt(key,bytes) {
  const stream=new Uint8Array(bytes.length);
  C.decodeGoogleEarthEnterpriseData(key.buffer,stream.buffer);
  return Uint8Array.from(bytes,(b,i)=>b^stream[i]);
}
const tile=i=>i===null?null:[i._bits,i.cnodeVersion,i.imageryVersion,i.terrainVersion,i.imageryProvider,i.terrainProvider,i.ancestorHasTerrain,i.terrainState??null];

export default async function workersTerrain() {
  await MeshoptEncoder.ready;
  const create=suite('createVerticesFromCesium3DTilesTerrain'),upsample=suite('upsampleVerticesFromCesium3DTilesTerrain');
  const gee=suite('createVerticesFromGoogleEarthEnterpriseBuffer'),packet=suite('decodeGoogleEarthEnterprisePacket');
  const createWorker=await loadWorker('createVerticesFromCesium3DTilesTerrain');
  const upsampleWorker=await loadWorker('upsampleVerticesFromCesium3DTilesTerrain');
  const geeWorker=await loadWorker('createVerticesFromGoogleEarthEnterpriseBuffer');
  const packetWorker=await loadWorker('decodeGoogleEarthEnterprisePacket');
  const rng=makeRng(0x7e44a1);

  // 3D Tiles terrain: grids of several sizes, normals, meshopt, exaggeration and levels.
  const ts=new C.GeographicTilingScheme();
  let k=0;
  for(const level of [3,11,17]) for(const size of [2,4,7]) for(const normals of [false,true]) for(const compression of [false,true]) {
    const tileX=level===3?13:5,tileY=level===3?2:3;
    const rectangle=ts.tileXYToRectangle(tileX,tileY,level);
    const g=gridGltf(rng,rectangle,size,normals,compression);
    const exaggeration=k%3===2?2.5:1,webMercatorT=k%4!==3,minimumHeight=-15,maximumHeight=420;
    const options={rectangle,normals,webMercatorT,exaggeration,exaggerationRelativeHeight:k%2?30:0,minimumHeight,maximumHeight,skirtHeight:k%5?40:0};
    const parameters=()=>({gltf:bufferGltf(g),ellipsoid:C.Ellipsoid.WGS84,rectangle,hasVertexNormals:normals,hasWebMercatorT:webMercatorT,
      minimumHeight,maximumHeight,boundingSphere:new C.BoundingSphere(C.Cartesian3.fromRadians(rectangle.west,rectangle.south),9000),
      orientedBoundingBox:C.OrientedBoundingBox.fromRectangle(rectangle,minimumHeight,maximumHeight),horizonOcclusionPoint:new C.Cartesian3(0.5,0.25,0.1),
      skirtHeight:options.skirtHeight,exaggeration,exaggerationRelativeHeight:options.exaggerationRelativeHeight});
    await create.add('create',[g.json,g.buffers,options],async()=>{
      const {result,transfers}=await invoke(createWorker,parameters());
      return {...meshProxy(result),transferCount:transfers.length};
    });
    // Feed the worker's own output back as the parent, as Cesium3DTilesTerrainData does.
    let parent;try{({result:parent}=await invoke(createWorker,parameters()));}catch(e){throw e;k++;continue;}
    for(const [east,north] of [[false,false],[true,false],[false,true],[true,true]]) {
      const child=ts.tileXYToRectangle(tileX*2+(east?1:0),tileY*2+(north?0:1),level+1);
      const childParameters={isEastChild:east,isNorthChild:north,parentVertices:new Float32Array(parent.verticesBuffer.slice(0)),
        parentIndices:new (parent.verticesBuffer.byteLength/4/parent.encoding.stride<=65535?Uint16Array:Uint32Array)(parent.indicesBuffer.slice(0)),
        parentVertexCountWithoutSkirts:parent.vertexCountWithoutSkirts,parentIndexCountWithoutSkirts:parent.indexCountWithoutSkirts,
        parentMinimumHeight:minimumHeight,parentMaximumHeight:maximumHeight,parentEncoding:parent.encoding,rectangle:child,
        ellipsoid:C.Ellipsoid.WGS84,skirtHeight:options.skirtHeight*0.5};
      await upsample.add('upsample',[g.json,g.buffers,options,east,north,[child.west,child.south,child.east,child.north]],async()=>{
        const {result,transfers}=await invoke(upsampleWorker,childParameters);
        return {...meshProxy(result),transferCount:transfers.length};
      });
    }
    k++;
  }
  // Rejected inputs: a missing position accessor and an out-of-range child index.
  {
    const rectangle=ts.tileXYToRectangle(5,3,11),g=gridGltf(rng,rectangle,3,false,false);
    const broken=structuredClone(g.json);delete broken.meshes[0].primitives[0].attributes.POSITION;
    const options={rectangle,normals:false,webMercatorT:true,exaggeration:1,exaggerationRelativeHeight:0,minimumHeight:0,maximumHeight:10,skirtHeight:5};
    await create.add('create',[broken,g.buffers,options],async()=>{
      const copy={json:broken,buffers:g.buffers};
      return meshProxy((await invoke(createWorker,{gltf:bufferGltf(copy),ellipsoid:C.Ellipsoid.WGS84,rectangle,hasVertexNormals:false,hasWebMercatorT:true,
        minimumHeight:0,maximumHeight:10,boundingSphere:new C.BoundingSphere(),orientedBoundingBox:new C.OrientedBoundingBox(),horizonOcclusionPoint:new C.Cartesian3(),
        skirtHeight:5,exaggeration:1,exaggerationRelativeHeight:0})).result);
    });
  }

  // GEE terrain buffers.
  const geeScheme=new C.GeographicTilingScheme({numberOfLevelZeroTilesX:2,numberOfLevelZeroTilesY:2,rectangle:new C.Rectangle(-Math.PI,-Math.PI,Math.PI,Math.PI)});
  const geeCase=async(bytes,level,o)=>{
    const rectangle=geeScheme.tileXYToRectangle(3,2,level);
    await gee.add('create',[Array.from(bytes),level,o],async()=>{
      const center=C.Ellipsoid.WGS84.cartographicToCartesian(C.Rectangle.center(rectangle));
      const {result,transfers}=await invoke(geeWorker,{buffer:Uint8Array.from(bytes).buffer,
        nativeRectangle:geeScheme.tileXYToNativeRectangle(3,2,level),rectangle,relativeToCenter:center,ellipsoid:C.Ellipsoid.WGS84,
        skirtHeight:Math.min(40075.16/(1<<level)*8,1000),exaggeration:o.exaggeration,exaggerationRelativeHeight:o.exaggerationRelativeHeight,
        includeWebMercatorT:o.webMercatorT,negativeAltitudeExponentBias:32,negativeElevationThreshold:o.threshold});
      return {vertices:Array.from(new Float32Array(result.vertices)),indices:Array.from(new Uint16Array(result.indices)),
        numberOfAttributes:result.numberOfAttributes,minimumHeight:result.minimumHeight,maximumHeight:result.maximumHeight,
        boundingSphere3D:result.boundingSphere3D,orientedBoundingBox:obb(result.orientedBoundingBox),
        occludeePointInScaledSpace:result.occludeePointInScaledSpace,encoding:encoding(result.encoding),
        vertexCountWithoutSkirts:result.vertexCountWithoutSkirts,indexCountWithoutSkirts:result.indexCountWithoutSkirts,
        edges:[result.westIndicesSouthToNorth,result.southIndicesEastToWest,result.eastIndicesNorthToSouth,result.northIndicesWestToEast],
        transferCount:transfers.length};
    });
  };
  for(const level of [2,9,18]) for(const size of [2,3,6]) for(const negative of [false,true]) {
    const rectangle=geeScheme.tileXYToRectangle(3,2,level);
    await geeCase(geeTerrain(rng,rectangle,size,{negative}),level,{exaggeration:size===3?1.5:1,exaggerationRelativeHeight:negative?12:0,webMercatorT:level!==9,threshold:1e-12});
  }
  {
    const level=7,rectangle=geeScheme.tileXYToRectangle(3,2,level),o={exaggeration:1,exaggerationRelativeHeight:0,webMercatorT:true,threshold:1e-12};
    await geeCase(geeTerrain(rng,rectangle,4,{flat:true}),level,o);
    const good=geeTerrain(rng,rectangle,3);
    // An index past the quad's points reads `undefined`, which Uint16Array stores as 0.
    const outOfRange=Buffer.from(good);outOfRange.writeUInt16LE(200,4+44+9*6);
    await geeCase(outOfRange,level,o);
    await geeCase(good.subarray(0,good.length-3),level,o);
    const wrongSize=Buffer.from(good);wrongSize.writeUInt32LE(good.readUInt32LE(0)-2,0);
    await geeCase(wrongSize,level,o);
    const negativeCount=Buffer.from(good);negativeCount.writeInt32LE(-50,4+32);
    await geeCase(negativeCount,level,o);
    await geeCase(Buffer.alloc(0),level,o);
  }

  // Encrypted GEE packets.
  const key=Uint8Array.from({length:1024},(_,i)=>(i*37+11)&255);
  const packetCase=async(type,plain,{quadKey='',encrypted=true,...o}={})=>{
    const bytes=plain===null?Buffer.alloc(0):compressed(plain,o);
    const data=encrypted?encrypt(key,bytes):Uint8Array.from(bytes);
    await packet.add(type,[Array.from(data),Array.from(key),quadKey],async()=>{
      const {result,transfers}=await invoke(packetWorker,{type,buffer:data.slice().buffer,key:key.slice().buffer,quadKey});
      if(type==='Metadata') return {tiles:Object.fromEntries(Object.entries(result).map(([k,v])=>[k,tile(v)])),transferCount:transfers.length};
      if(type==='Terrain') return {meshes:result.map(b=>Array.from(new Uint8Array(b))),transferCount:transfers.length};
      if(type==='DbRoot') return {buffer:Array.from(new Uint8Array(result.buffer)),transferCount:transfers.length};
      return result;
    });
  };
  for(const [bits,quadKey] of [[[15,128,128,128,128],''],[[3,128,0],''],[[0],''],[[64+15,3,1,0,2,1,0],'21'],[[9,15,0,0,0,0,128],'0'],[[15,15],''],[[15,128,64],'']])
    await packetCase('Metadata',metadataPacket(bits),{quadKey});
  // Level-4 children of a deep packet: subtrees stop, leaves become null.
  await packetCase('Metadata',metadataPacket([1,1,1,1,1,64,1]),{quadKey:'3'});
  await packetCase('Metadata',metadataPacket([1,1,1,1,1]),{quadKey:'3'});
  await packetCase('Metadata',metadataPacket([15,128,128,128,128]),{swap:true});
  await packetCase('Metadata',metadataPacket([15,128,128,128,128]),{gzip:true});
  await packetCase('Metadata',metadataPacket([3,128,0]),{encrypted:false});
  for(const fault of [{magic:1},{type:2},{version:1},{instanceSize:16},{offsetFix:4},{sizeFix:8}])
    await packetCase('Metadata',metadataPacket([3,128,0],fault));
  await packetCase('Metadata',metadataPacket([3,128,0]),{sizeFix:1});
  for(const [sizes,trailing] of [[[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20],0],[Array(20).fill(0),9],[Array.from({length:20},(_,i)=>(i*13)%40),33]])
    await packetCase('Terrain',terrainPacket(rng,sizes,trailing));
  const truncated=terrainPacket(rng,Array(20).fill(3));
  await packetCase('Terrain',truncated.subarray(0,truncated.length-2));
  await packetCase('Terrain',terrainPacket(rng,Array(19).fill(3)));
  await packetCase('DbRoot',Buffer.from([1,2,3,250,0,9,8]));
  await packetCase('DbRoot',Buffer.alloc(0));
  await packetCase('DbRoot',Buffer.from([7]),{swap:true});
  const corrupt=compressed(Buffer.from([1,2,3,4]));corrupt.writeUInt32LE(0x12345678,0);
  await packet.add('DbRoot',[Array.from(encrypt(key,corrupt)),Array.from(key),''],async()=>(await invoke(packetWorker,{type:'DbRoot',buffer:encrypt(key,corrupt).buffer,key:key.slice().buffer,quadKey:''})).result);
  await packet.add('DbRoot',[[1,2,3,4,5,6,7,8],[1,2,3],''],async()=>(await invoke(packetWorker,{type:'DbRoot',buffer:new Uint8Array([1,2,3,4,5,6,7,8]).buffer,key:new Uint8Array(3).buffer,quadKey:''})).result);
  return [create,upsample,gee,packet];
}
