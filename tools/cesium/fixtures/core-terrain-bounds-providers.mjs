import { ApproximateTerrainHeights, CustomHeightmapTerrainProvider, Ellipsoid, EllipsoidTerrainProvider, GeographicTilingScheme, Rectangle, Resource, TerrainProvider, WebMercatorTilingScheme } from "@cesium/engine";
import { readFileSync } from "node:fs";
import { enc, generators, makeRng } from "./lib.mjs";
const suite=module=>({module,cases:[],async add(f,args,fn){let r;try{r=enc(await fn(...args));}catch(e){r={throws:String(e.message??e),kind:e.name??"Error"};}this.cases.push({f,a:args.map(enc),r});}});
const types={Int8Array,Uint8Array,Int16Array,Uint16Array,Int32Array,Uint32Array,Float32Array,Float64Array};
const makeScheme=o=>o.scheme?new (o.scheme==="mercator"?WebMercatorTilingScheme:GeographicTilingScheme)({ellipsoid:Ellipsoid[o.schemeEllipsoid??"WGS84"],numberOfLevelZeroTilesX:o.x??(o.scheme==="mercator"?1:2),numberOfLevelZeroTilesY:o.y??1}):undefined;
const providerMeta=p=>[p.credit?.html??null,p.hasWaterMask,p.hasVertexNormals,p.availability??null,p.tilingScheme.ellipsoid.radii,p.tilingScheme.getNumberOfXTilesAtLevel(0),p.tilingScheme.getNumberOfYTilesAtLevel(0),p.errorEvent===p.errorEvent,[0,1,5,30,31,32,33,64].map(l=>p.getLevelMaximumGeometricError(l)),p.getTileDataAvailable(1,2,3)??null,p.loadTileDataAvailability(1,2,3)??null];
const dataMeta=d=>[d._buffer.constructor.name,d._buffer,d._width,d._height,d.childTileMask,d.credits??null,d.waterMask??null,d.wasCreatedByUpsampling()];

export default async function coreTerrainBoundsProviders(){
  const heights=suite("ApproximateTerrainHeights"),ellipsoid=suite("EllipsoidTerrainProvider"),custom=suite("CustomHeightmapTerrainProvider"),g=generators(makeRng(2300));
  const table=JSON.parse(readFileSync(new URL("../node_modules/@cesium/engine/Source/Assets/approximateTerrainHeights.json",import.meta.url),"utf8"));
  const oldFetch=Resource.fetchJson,initial=ApproximateTerrainHeights.initialized;
  Resource.fetchJson=()=>Promise.resolve(table);
  try{const first=ApproximateTerrainHeights.initialize(),second=ApproximateTerrainHeights.initialize();await first;await heights.add("initialize",[],()=>[initial,first===second,ApproximateTerrainHeights.initialized]);}
  finally{Resource.fetchJson=oldFetch;}
  const originalTable=ApproximateTerrainHeights._terrainHeights;
  const prefix=Rectangle.fromDegrees(.1,10.1,.2,10.2);
  const rectangles=[Rectangle.MAX_VALUE,Rectangle.fromDegrees(170,-10,-170,10),Rectangle.fromDegrees(-170,-80,-160,-70),Rectangle.fromDegrees(.1,10.1,.2,10.2),Rectangle.fromDegrees(0,0,0,0),Rectangle.fromDegrees(1,90,1,90),Rectangle.fromDegrees(-180,-90,-180,-90),new Rectangle(4,-2,4.1,2),new Rectangle(4,0,4.1,.1)];
  for(let i=0;i<64;i++){const west=g.uniform(-179,170),south=g.uniform(-85,75),size=g.pick([.001,.1,1,5,30]);rectangles.push(Rectangle.fromDegrees(west,south,west+size,south+Math.min(size,89-south)));}
  for(let i=0;i<rectangles.length;i++)for(const name of ["WGS84","MOON","UNIT_SPHERE"]){
    for(const mode of i<8?[0,1,2]:[0])await heights.add("query",[rectangles[i],name,mode],(r,name,mode)=>{
      ApproximateTerrainHeights._terrainHeights=mode===1?{}:mode===2?{"0-0-0":[-250000,18000],"6-64-28":[-300,12345]}:table;
      try{ApproximateTerrainHeights.getMinimumMaximumHeights(prefix,Ellipsoid.WGS84);return[ApproximateTerrainHeights.getMinimumMaximumHeights(r,Ellipsoid[name]),ApproximateTerrainHeights.getBoundingSphere(r,Ellipsoid[name])];}
      finally{ApproximateTerrainHeights._terrainHeights=originalTable;}
    });
  }
  const configurations=[{}, {ellipsoid:"MOON"},{scheme:"geographic",ellipsoid:"MOON",schemeEllipsoid:"UNIT_SPHERE",x:3,y:2},{scheme:"mercator",ellipsoid:"UNIT_SPHERE",schemeEllipsoid:"MOON"},{scheme:"mercator",x:2,y:2}];
  for(const o of configurations)for(const quality of [0,.25,1])await ellipsoid.add("provider",[o,quality],async(o,quality)=>{
    const previous=TerrainProvider.heightmapTerrainQuality;let p;
    try{TerrainProvider.heightmapTerrainQuality=quality;p=new EllipsoidTerrainProvider({tilingScheme:makeScheme(o),ellipsoid:o.ellipsoid?Ellipsoid[o.ellipsoid]:undefined});}
    finally{TerrainProvider.heightmapTerrainQuality=previous;}
    const first=await p.requestTileGeometry(0,0,0),second=await p.requestTileGeometry(10,20,31);
    return[providerMeta(p),dataMeta(first),dataMeta(second),first._buffer===second._buffer];
  });
  for(let i=0;i<44;i++){
    const width=2+i%4,height=2+(i>>1)%4,type=i%9===8?"Array":Object.keys(types)[i%9];
    const values=Array.from({length:width*height},(_,j)=>j%4===0?-300.25:j%4===1?65540.5:g.uniform(-1000,1000));
    const o={...configurations[i%configurations.length],width,height,type,values,mode:["ready","async","defer","reject"][i%4],credit:i%2?null:"Terrain <b>data</b>",quality:i%3===0?0:.25};
    await custom.add("provider",[o],async o=>{
      const calls=[],callback=(x,y,level)=>{
        calls.push([x,y,level]);if(o.mode==="defer"&&x===1)return undefined;
        if(o.mode==="reject")return Promise.reject(new Error("terrain callback failed"));
        const values=o.values.map(v=>v+x-y+level),buffer=o.type==="Array"?values:new types[o.type](values);
        return o.mode==="async"?Promise.resolve(buffer):buffer;
      };
      const previous=TerrainProvider.heightmapTerrainQuality;let p;
      try{TerrainProvider.heightmapTerrainQuality=o.quality;p=new CustomHeightmapTerrainProvider({callback,width:o.width,height:o.height,tilingScheme:makeScheme(o),ellipsoid:o.ellipsoid?Ellipsoid[o.ellipsoid]:undefined,credit:o.credit??undefined});}
      finally{TerrainProvider.heightmapTerrainQuality=previous;}
      const results=[];for(const [x,y,l] of [[0,0,0],[1,2,3],[3,4,31]]){
        const promise=p.requestTileGeometry(x,y,l);if(!promise){results.push(null);continue;}
        try{results.push(dataMeta(await promise));}catch(e){results.push(["error",e.message]);}
      }
      return[providerMeta(p),p.width,p.height,calls,results];
    });
  }
  return [heights,ellipsoid,custom];
}
