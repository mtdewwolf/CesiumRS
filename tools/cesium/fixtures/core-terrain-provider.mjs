// Terrain grid topology and spatial availability against the reference engine.
import { Cartographic, Ellipsoid, GeographicTilingScheme, Rectangle, TerrainProvider, TileAvailability, WebMercatorTilingScheme } from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const typed = a => [a.constructor.name, Array.from(a)];
const topology = a => ({ indices: typed(a.indices), west: a.westIndicesSouthToNorth, south: a.southIndicesEastToWest, east: a.eastIndicesNorthToSouth, north: a.northIndicesWestToEast, count: a.indexCountWithoutSkirts ?? null });
const digest = a => {
  let sum = 0, max = 0;
  for (const i of a) { sum += i; max = Math.max(max, i); }
  return [a.constructor.name, a.length, sum, max, Array.from(a.slice(0, 24)), Array.from(a.slice(-24))];
};

function providerSuite() {
  const s = suite("TerrainProvider"), g = generators(makeRng(1800));
  const sizes = [[1,1],[1,5],[5,1],[2,2],[3,5],[17,9],...Array.from({length:15},()=>[2+Math.floor(g.rng()*12),2+Math.floor(g.rng()*12)])];
  for (const [width,height] of sizes) {
    s.add("grid",[width,height],(w,h)=>typed(TerrainProvider.getRegularGridIndices(w,h)));
    s.add("edges",[width,height],(w,h)=>topology(TerrainProvider.getRegularGridIndicesAndEdgeIndices(w,h)));
    s.add("skirts",[width,height],(w,h)=>topology(TerrainProvider.getRegularGridAndSkirtIndicesAndEdgeIndices(w,h)));
    s.add("cache",[width,height],(w,h)=>[
      TerrainProvider.getRegularGridIndices(w,h)===TerrainProvider.getRegularGridIndices(w,h),
      TerrainProvider.getRegularGridIndicesAndEdgeIndices(w,h)===TerrainProvider.getRegularGridIndicesAndEdgeIndices(w,h),
      TerrainProvider.getRegularGridAndSkirtIndicesAndEdgeIndices(w,h)===TerrainProvider.getRegularGridAndSkirtIndicesAndEdgeIndices(w,h),
      TerrainProvider.getRegularGridIndicesAndEdgeIndices(w,h).indices===TerrainProvider.getRegularGridIndices(w,h),
    ]);
  }
  for (const [width,height] of [[255,257],[256,256],[255,255],[257,257]]) {
    s.add("largeGrid",[width,height],(w,h)=>[digest(TerrainProvider.getRegularGridIndices(w,h)),digest(TerrainProvider.getRegularGridAndSkirtIndicesAndEdgeIndices(w,h).indices)]);
  }
  for (let i=0;i<30;i++) {
    const edges=Array.from({length:4},()=>Array.from({length:1+Math.floor(g.rng()*7)},()=>Math.floor(g.rng()*1000)));
    const vertexCount=i<20?2000:65530;
    const caps=i%2===0;
    const type=i%3===0?"Uint8Array":i%3===1?"Uint16Array":"Uint32Array";
    s.add("addSkirts",[edges,vertexCount,caps,type,3],(e,v,c,t,offset)=>{
      const count=TerrainProvider.getSkirtVertexCount(...e);
      const length=c?TerrainProvider.getSkirtIndexCountWithFilledCorners(count):TerrainProvider.getSkirtIndexCount(count);
      const buffer=new ({Uint8Array,Uint16Array,Uint32Array}[t])(offset+length+4).fill(7);
      TerrainProvider[c?"addSkirtIndicesWithFilledCorners":"addSkirtIndices"](...e,v,buffer,offset);
      return [count,TerrainProvider.getSkirtIndexCount(count),TerrainProvider.getSkirtIndexCountWithFilledCorners(count),typed(buffer)];
    });
  }
  for (const name of ["WGS84","MOON","UNIT_SPHERE"]) {
    for (const width of [0,1,65,256]) {
      for (const quality of [null,0,0.5,1]) s.add("geometricError",[name,width,2,quality],(e,w,n,q)=>{
        const previous=TerrainProvider.heightmapTerrainQuality;
        try { TerrainProvider.heightmapTerrainQuality=q??0.25; return TerrainProvider.getEstimatedLevelZeroGeometricErrorForAHeightmap(Ellipsoid[e],w,n); }
        finally { TerrainProvider.heightmapTerrainQuality=previous; }
      });
    }
  }
  return s;
}

function scheme(o) {
  if(o.kind==="mercator") return new WebMercatorTilingScheme({numberOfLevelZeroTilesX:o.x??1,numberOfLevelZeroTilesY:o.y??1});
  return new GeographicTilingScheme({rectangle:o.rectangle?new Rectangle(...o.rectangle):undefined,numberOfLevelZeroTilesX:o.x??2,numberOfLevelZeroTilesY:o.y??1});
}

function availabilitySuite() {
  const s=suite("TileAvailability"),g=generators(makeRng(1801));
  const add=(o,max,ranges,points,rectangles,tiles)=>s.add("availability",[o,max,ranges,points,rectangles,tiles],(o,max,ranges,points,rectangles,tiles)=>{
    const a=new TileAvailability(scheme(o),max), snapshots=[];
    const query=()=>[
      points.map(p=>a.computeMaximumLevelAtPosition(new Cartographic(...p))),
      rectangles.map(r=>a.computeBestAvailableLevelOverRectangle(new Rectangle(...r))),
      tiles.map(([l,x,y])=>[a.isTileAvailable(l,x,y),a.computeChildMaskForTile(l,x,y)]),
    ];
    snapshots.push(query());
    for (const r of ranges){a.addAvailableTileRange(...r);snapshots.push(query());}
    return snapshots;
  });
  const rectangle = (west,south,east,north)=>[west,south,east,north].map(v=>v*Math.PI/180);
  const points=[[0,0,0],[-Math.PI,0,0],[Math.PI,0,0],[0,Math.PI/2,0],[0,-Math.PI/2,0],[4,0,0],[-Math.PI/2,0,100],[-1,0.3,0],[1,-0.3,0]];
  const rectangles=[rectangle(-180,-90,180,90),rectangle(-170,0,-100,40),rectangle(100,-40,170,0),rectangle(170,-10,-170,10),rectangle(0,0,0,0),rectangle(-20,-20,20,20)];
  // A sparse node checks only its first matching child at shared boundaries.
  // Allocating its fourth child switches Cesium to visiting all matches.
  add({kind:"geographic"},6,[[0,0,0,0,0],[3,0,2,0,2],[3,2,2,2,2]],
    [[-Math.PI,Math.PI/4,0],[-3*Math.PI/4,Math.PI/4,0]],[],[]);
  for(const kind of ["geographic","mercator"]){
    const o={kind},ts=scheme(o),nx=ts.getNumberOfXTilesAtLevel(0),ny=ts.getNumberOfYTilesAtLevel(0);
    const tiles=[];
    for(let l=0;l<5;l++)for(let j=0;j<5;j++)tiles.push([l,Math.floor(g.rng()*ts.getNumberOfXTilesAtLevel(l)),Math.floor(g.rng()*ts.getNumberOfYTilesAtLevel(l))]);
    const ranges=[[2,0,0,0,0],[0,0,0,nx-1,ny-1],[0,0,0,0,0],[3,0,0,3,3],[1,0,0,nx*2-1,ny*2-1],[4,0,0,1,1],[2,0,0,nx*4-1,ny*4-1]];
    for(const max of [0,1,2,4,8])add(o,max,ranges,points,rectangles,tiles);
    // Boundary points on fine tiles, out-of-order levels, overlapping ranges,
    // duplicate ranges and disconnected level-zero roots.
    for(let i=0;i<18;i++){
      const ranges=[];
      for(let j=0;j<8;j++){
        const l=j===0?0:Math.floor(g.rng()*5),xn=ts.getNumberOfXTilesAtLevel(l),yn=ts.getNumberOfYTilesAtLevel(l);
        const x=Math.floor(g.rng()*xn),y=Math.floor(g.rng()*yn);
        ranges.push([l,x,y,Math.min(x+Math.floor(g.rng()*4),xn-1),Math.min(y+Math.floor(g.rng()*4),yn-1)]);
      }
      const l=2, r=ts.tileXYToRectangle(0,0,l);
      const ps=[...points,[r.west,r.south,0],[r.east,r.north,0],...Array.from({length:12},()=>[g.uniform(-Math.PI,Math.PI),g.uniform(-1.5,1.5),0])];
      const rs=[...rectangles,[r.west,r.south,r.east,r.north]];
      add(o,6,ranges,ps,rs,tiles);
    }
  }
  // Non-default root layouts and a regional geographic tiling scheme.
  for(const o of [{kind:"geographic",x:1,y:1},{kind:"geographic",x:2,y:2,rectangle:rectangle(-40,-30,60,70)},{kind:"mercator",x:2,y:2}]){
    const ts=scheme(o),nx=ts.getNumberOfXTilesAtLevel(0),ny=ts.getNumberOfYTilesAtLevel(0);
    const ranges=[[0,0,0,nx-1,ny-1],[2,0,0,nx*4-1,ny*4-1],[3,0,0,1,1]];
    add(o,5,ranges,points,rectangles,[[0,0,0],[1,0,0],[2,0,0],[3,0,0]]);
  }
  return s;
}

export default function coreTerrainProvider(){return [providerSuite(),availabilitySuite()];}
