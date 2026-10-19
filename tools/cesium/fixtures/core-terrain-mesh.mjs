// Exercise the real Cesium worker through its message adapter, without a
// browser. TaskProcessor scheduling is the sole host adapter; calculations,
// refinement, traversal, encoding and picking are the pinned engine's code.
import { AxisAlignedBoundingBox, BoundingSphere, Cartesian2, Cartesian3, Cartographic, Ellipsoid, GeographicProjection, Matrix4, OrientedBoundingBox, Ray, Rectangle, TaskProcessor, TerrainEncoding, TerrainMesh, TerrainPicker, TerrainProvider, Transforms, WebMercatorProjection } from "@cesium/engine";
import { enc, generators, makeRng } from "./lib.mjs";

const c3 = a => new Cartesian3(...a);
const box = a => new AxisAlignedBoundingBox(c3(a[0]),c3(a[1]));
const projection = merc => merc ? new WebMercatorProjection() : new GeographicProjection();
const makeEncoding = o => new TerrainEncoding(c3(o.center), o.box ? box(o.box) : undefined,
  o.minimum, o.maximum, o.frame ? Matrix4.fromArray(o.frame) : undefined,
  false, false, o.geo ?? false, o.exaggeration ?? 1, o.relative ?? 0);
const encode = (e, points) => {
  const b = new Float32Array(e.stride * points.length);
  points.forEach(([p,uv,h,n],i)=>e.encode(b,i*e.stride,c3(p),new Cartesian2(...uv),h,undefined,undefined,c3(n)));
  return b;
};
const nodeCount = n => 1+n.children.reduce((s,c)=>s+nodeCount(c),0);
const rootShape = p => [p.needsRebuild,nodeCount(p._rootNode)];
const makeSuite = module => ({module,cases:[],async add(f,args,fn){
  let r;
  try { r=enc(await fn(...args)); } catch(e) { r={throws:String(e.message??e),kind:e.name??"Error"}; }
  this.cases.push({f,a:args.map(enc),r});
}});

export default async function coreTerrainMesh() {
  const originalSelf=globalThis.self, originalPost=globalThis.postMessage;
  globalThis.self={};
  const worker=(await import("@cesium/engine/Source/Workers/incrementallyBuildTerrainPicker.js")).default;
  globalThis.self=originalSelf;
  const queue=[];
  const previousSchedule=TaskProcessor.prototype.scheduleTask;
  const process = async parameters => {
    let response;
    globalThis.self=worker;
    globalThis.postMessage=r=>{response=r;};
    try {
      await worker.onmessage({data:{id:0,parameters,canTransferArrayBuffer:false}});
      if(response.error) throw new Error(JSON.stringify(response.error));
      return response.result;
    } finally { globalThis.self=originalSelf; globalThis.postMessage=originalPost; }
  };
  TaskProcessor.prototype.scheduleTask=function(parameters) {
    return new Promise((resolve,reject)=>queue.push(async()=>{
      try {resolve(await process(parameters));}catch(e){reject(e);}
    }));
  };
  const drain=async()=>{while(queue.length){await queue.shift()();await Promise.resolve();}};
  const picker=makeSuite("TerrainPicker"),mesh=makeSuite("TerrainMesh"),distribution=makeSuite("incrementallyBuildTerrainPicker");
  const g=generators(makeRng(1900));
  try {
    // Standalone affine tiles with overlapping triangles, culling and rays
    // outside/behind/parallel to the cube. Repeated hits deepen to level 3.
    for(let i=0;i<36;i++) {
      const frame=i===0?Matrix4.clone(Matrix4.IDENTITY):g.affine4();
      const scale=i%2===0?1000:10000;
      const local=[[-scale/2,-scale/2,0],[scale/2,-scale/2,0],[scale/2,scale/2,0],[-scale/2,scale/2,0],[-scale/2,-scale/2,scale/5],[scale/2,-scale/2,scale/5],[scale/2,scale/2,scale/5]];
      const points=local.map((p,j)=>{
        const world=Matrix4.multiplyByPoint(frame,c3(p),new Cartesian3());
        return [[world.x,world.y,world.z],[j%2,j%3/2],p[2],[0,0,1]];
      });
      const o={center:Array.from([frame[12],frame[13],frame[14]]),box:[[-scale/2,-scale/2,-scale/2],[scale/2,scale/2,scale/2]],minimum:-scale/2,maximum:scale/2,frame:Array.from(frame)};
      const transform=Matrix4.multiplyByScale(frame,new Cartesian3(scale,scale,scale),new Matrix4());
      const rays=[[0,0,scale],[scale/5,-scale/5,scale],[0,0,-scale],[2*scale,0,scale],[0,0,0],[-scale/2,-scale/2,scale]];
      const operations=[...Array(5).fill(0),1,2,3,4,5,0].map((r,j)=>{
        const origin=Matrix4.multiplyByPoint(frame,c3(rays[r]),new Cartesian3());
        const direction=Matrix4.multiplyByPointAsVector(frame,new Cartesian3(j===8?1:0,0,j===8?0:-1),new Cartesian3());
        return [[origin.x,origin.y,origin.z],[direction.x,direction.y,direction.z],j%3===2,j===10];
      });
      await picker.add("pick",[o,points,[0,1,2,0,2,3,4,5,6],Array.from(transform),operations],async(o,points,indices,transform,operations)=>{
        const e=makeEncoding(o),vertices=encode(e,points),p=new TerrainPicker(vertices,new Uint32Array(indices),e);
        const results=[rootShape(p)];
        for(const [origin,direction,cull,rebuild] of operations){
          if(rebuild)p.needsRebuild=true;
          const hit=p.rayIntersect(new Ray(c3(origin),c3(direction)),Matrix4.fromArray(transform),cull,3,projection(false));
          await drain();results.push([hit??null,rootShape(p)]);
        }
        return results;
      });
    }
    // World tiles, both quantization modes, heights and relative exaggeration.
    for(let i=0;i<48;i++) {
      const west=i<4?[179,-180,-10,40][i]:g.uniform(-160,150), south=i<4?-10:g.uniform(-75,65);
      const rect=Rectangle.fromDegrees(west,south,west+(i%2?0.01:2),south+(i%2?0.01:2));
      if(i===0)rect.east=Rectangle.fromDegrees(-179,0,-179,0).east;
      const minimum=i%4===0?100:-50, maximum=i%4===0?100:800;
      const center=Ellipsoid.WGS84.cartographicToCartesian(Rectangle.center(rect));
      const frame=Transforms.eastNorthUpToFixedFrame(center);
      const width=i%2?2000:20000;
      const o={center:[center.x,center.y,center.z],box:[[-width,-width,-1000],[width,width,1000]],minimum,maximum,frame:Array.from(frame),geo:true,exaggeration:[1,0,2.5][i%3],relative:30};
      const points=[];
      for(let y=0;y<3;y++)for(let x=0;x<3;x++) {
        const lon=rect.west+Rectangle.computeWidth(rect)*x/2,lat=rect.north+(rect.south-rect.north)*y/2;
        const h=minimum+(maximum-minimum)*(x+y)/4;
        const p=Ellipsoid.WGS84.cartographicToCartesian(new Cartographic(lon,lat,h));
        const n=Ellipsoid.WGS84.geodeticSurfaceNormal(p);
        points.push([[p.x,p.y,p.z],[x/2,1-y/2],h,[n.x,n.y,n.z]]);
      }
      await mesh.add("mesh",[o,points,[rect.west,rect.south,rect.east,rect.north],i%2===0,i%3===0,i%4===0],async(o,points,rect,merc,withBox,defaultStride)=>{
        const e=makeEncoding(o),vertices=encode(e,points),topology=TerrainProvider.getRegularGridIndicesAndEdgeIndices(3,3);
        const m=new TerrainMesh(c3(o.center),vertices,topology.indices,24,9,o.minimum,o.maximum,new Rectangle(...rect),new BoundingSphere(c3(o.center),50000),new Cartesian3(1,2,3),defaultStride?undefined:e.stride,withBox?new OrientedBoundingBox():undefined,e,topology.westIndicesSouthToNorth,topology.southIndicesEastToWest,topology.eastIndicesNorthToSouth,topology.northIndicesWestToEast);
        const result=[[m.center,m.stride,m.indices,m.indexCountWithoutSkirts,m.vertexCountWithoutSkirts,m.minimumHeight,m.maximumHeight,m.rectangle,m.boundingSphere3D,m.occludeePointInScaledSpace,[m.westIndicesSouthToNorth,m.southIndicesEastToWest,m.eastIndicesNorthToSouth,m.northIndicesWestToEast]],enc(m.getTransform(undefined,projection(merc)))];
        const summary=()=>[m.orientedBoundingBox?[m.orientedBoundingBox.center,m.orientedBoundingBox.halfAxes]:null,rootShape(m._terrainPicker)];
        for(const mode of [3,3,3,3,2,2,1,0,3]){
          const proj=projection(merc),target=e.getExaggeratedPosition(vertices,4);
          let origin,direction;
          if(mode===3){const n=Ellipsoid.WGS84.geodeticSurfaceNormal(target);origin=Cartesian3.add(target,Cartesian3.multiplyByScalar(n,10000,new Cartesian3()),new Cartesian3());direction=Cartesian3.negate(n,new Cartesian3());}
          else{const p=proj.project(Ellipsoid.WGS84.cartesianToCartographic(target));origin=new Cartesian3(p.z+10000,p.x+(i%2?2*Math.PI*Ellipsoid.WGS84.maximumRadius:0),p.y);direction=new Cartesian3(-1,0,0);}
          const hit=m.pick(new Ray(origin,direction),i%2===0,mode,proj);
          await drain();result.push(enc([m.getTransform(mode,proj),hit??null,summary()]));
        }
        e.exaggeration=4;e.exaggerationRelativeHeight=-20;
        m.updateExaggeration(999,888);result.push(enc([m.getTransform(3,projection(merc)),summary()]));
        m.updateSceneMode(2);result.push(enc([m.getTransform(2,projection(merc)),summary()]));
        return result;
      });
    }
    // Direct worker validation includes degenerate AABBs and clamping of
    // triangles well outside the artificial unit-cube height interval.
    const boxes=[[-.5,-.5,0,0],[0,-.5,.5,0],[-.5,0,0,.5],[0,0,.5,.5]].map(([w,s,e,n])=>[[w,s,-.5],[e,n,.5]]);
    for(let i=0;i<40;i++) {
      const transform=i%2?g.affine4():Matrix4.IDENTITY;
      const triangles=Array.from({length:i%11},()=>Array.from({length:3},()=>[g.uniform(-2,2),g.uniform(-2,2),g.uniform(-5,5)]));
      await distribution.add("distribute",[boxes,Array.from(transform),triangles],async(boxes,inverse,triangles)=>{
        const result=await process({aabbs:new Float64Array(boxes.flat(2)),inverseTransform:new Float64Array(inverse),triangleIndices:new Uint32Array(triangles.map((_,j)=>j+10)),trianglePositions:new Float64Array(triangles.flat(2))});
        return result.intersectingTrianglesArrays.map(b=>Array.from(new Uint32Array(b)));
      });
    }
    return [picker,mesh,distribution];
  } finally { TaskProcessor.prototype.scheduleTask=previousSchedule;globalThis.self=originalSelf;globalThis.postMessage=originalPost; }
}
