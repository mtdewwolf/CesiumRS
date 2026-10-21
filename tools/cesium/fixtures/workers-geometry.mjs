// Exercise the actual Workers entry points, rather than only their Core factories.
import * as C from '@cesium/engine';
import {readFileSync} from 'node:fs';
import {enc} from './lib.mjs';
import {geometryEnc} from './core-geometries.mjs';
import PrimitivePipeline from '@cesium/engine/Source/Scene/PrimitivePipeline.js';
const suite = module => ({module,cases:[],async add(f,a,fn) {
  let r; try {r=enc(await fn());} catch(e) {r={throws:e.message,kind:e.name};}
  this.cases.push({f,a:enc(a),r});
}});
const names = [
  'Box','BoxOutline','Circle','CircleOutline','CoplanarPolygon','CoplanarPolygonOutline',
  'Corridor','CorridorOutline','Cylinder','CylinderOutline','Ellipse','EllipseOutline',
  'Ellipsoid','EllipsoidOutline','Frustum','FrustumOutline','GroundPolyline','Plane',
  'PlaneOutline','Polygon','PolygonOutline','Polyline','PolylineVolume','PolylineVolumeOutline',
  'Rectangle','RectangleOutline','SimplePolyline','Sphere','SphereOutline','Wall','WallOutline',
].map(n=>n+'Geometry');

function description(name,k) {
  const ellipsoid=k===2?C.Ellipsoid.MOON:C.Ellipsoid.WGS84;
  const positions=C.Cartesian3.fromDegreesArrayHeights(
    k===3?[179,20,0,-179,20.4,30,-178.5,21,20]:[-75,35,10,-74.7,35.2,80,-74.5,35.1,20],ellipsoid);
  const options={
    ellipsoid, positions, center:positions[0],
    vertexFormat:k===1?C.VertexFormat.POSITION_ONLY:k===3?C.VertexFormat.POSITION_NORMAL_AND_ST:C.VertexFormat.ALL,
    granularity:0.15, height:k===0?0:100, extrudedHeight:k===1?-50:undefined,
    offsetAttribute:k===2?C.GeometryOffsetAttribute.ALL:undefined,
    radius:k===4?0:25000, semiMajorAxis:k===4?0:30000, semiMinorAxis:k===4?0:18000,
    rotation:0.3, stRotation:0.2,
    minimum:new C.Cartesian3(-1,-2,-3), maximum:k===4?new C.Cartesian3(-1,-2,-3):new C.Cartesian3(3,4,5),
    length:k===4?0:4,topRadius:1,bottomRadius:2,slices:8,
    numberOfVerticalLines:4,
    radii:new C.Cartesian3(k===4?0:2,3,4), innerRadii:k===1?new C.Cartesian3(1,1,1):undefined,
    stackPartitions:4,slicePartitions:5,subdivisions:8,
    minimumClock:k===1?0.5:0, maximumClock:k===1?4:2*Math.PI,
    minimumCone:k===1?0.2:0,maximumCone:k===1?2.5:Math.PI,
    rectangle:C.Rectangle.fromDegrees(k===3?179:-75,34,k===3?-179:-74,35),
    width:k===4?0:500,cornerType:k%3,arcType:k===2?C.ArcType.RHUMB:C.ArcType.GEODESIC,
    loop:k===1,perPositionHeight:k===2,
    shapePositions:[new C.Cartesian2(-2,-1),new C.Cartesian2(2,-1),new C.Cartesian2(2,1),new C.Cartesian2(-2,1)],
    polylinePositions:positions,
    polygonHierarchy:new C.PolygonHierarchy(k===4?[positions[0],positions[0]]:positions),
    minimumHeights:k===1?[0,10,20]:undefined,maximumHeights:k===1?[100,110,120]:undefined,
  };
  if(name.startsWith('Coplanar')) options.polygonHierarchy=new C.PolygonHierarchy(
    k===4?[C.Cartesian3.ZERO,C.Cartesian3.ZERO]:[new C.Cartesian3(0,0,2),new C.Cartesian3(3,0,2),new C.Cartesian3(2,3,2)]);
  if(name.startsWith('Frustum')) {
    options.frustum=k%2?new C.OrthographicFrustum({width:3,aspectRatio:1.5,near:1,far:10}):
      new C.PerspectiveFrustum({fov:0.8,aspectRatio:1.5,near:1,far:10});
    options.origin=new C.Cartesian3(10,20,30);
    options.orientation=C.Quaternion.fromAxisAngle(C.Cartesian3.UNIT_Z,0.4);
  }
  if(name==='GroundPolylineGeometry' && k===4) options.positions=[positions[0],positions[0]];
  if(name==='PolylineGeometry' && k===4) options.positions=[positions[0],positions[0]];
  if(name.includes('PolylineVolume') && k===4) options.polylinePositions=[positions[0],positions[0]];
  if(name.startsWith('Corridor') && k===4) options.positions=[positions[0],positions[0]];
  if(name.startsWith('Wall') && k===4) options.positions=[positions[0],positions[0]];
  if (name.startsWith('Polygon') && options.perPositionHeight) options.height=undefined;
  const result=new C[name](options);
  if(name==='GroundPolylineGeometry') {
    C.GroundPolylineGeometry.setProjectionAndEllipsoid(result,k===1?new C.WebMercatorProjection(ellipsoid):new C.GeographicProjection(ellipsoid));
    result._scene3DOnly=k===2;
    result.granularity=100000;
  }
  if(name.startsWith('Frustum') && k===2) result._drawNearPlane=false;
  return result;
}
function pack(name,value,offset) {
  const result=new Float64Array(offset+(value.packedLength??C[name].packedLength)).fill(91);
  C[name].pack(value,result,offset);
  return result;
}

export default async function workersGeometry() {
  const previousHeights=C.ApproximateTerrainHeights._terrainHeights;
  const previousSelf=globalThis.self;
  const previousPost=globalThis.postMessage;
  const previousInit=C.ApproximateTerrainHeights._initPromise;
  const suites=[];
  try {
    C.ApproximateTerrainHeights._terrainHeights=JSON.parse(readFileSync(new URL('../../../assets/approximateTerrainHeights.json',import.meta.url),'utf8'));
    C.ApproximateTerrainHeights._initPromise=Promise.resolve();
    for(const name of names) {
      const s=suite('create'+name);
      const worker=(await import('@cesium/engine/Source/Workers/create'+name+'.js')).default;
      for(let k=0;k<5;k++) {
        for(const offset of [null,0,3]) {
          const value=description(name,k);
          const packed=pack(name,value,offset??0);
          await s.add(offset===null?'direct':'packed',[packed,offset],async()=>geometryEnc(
            await worker(offset===null?value:packed,offset??undefined)));
        }
      }
      suites.push(s);
    }
    let response,transfers;
    globalThis.self={};
    globalThis.postMessage=(r,t=[])=>{response=r;transfers=t;};
    const worker=(await import('@cesium/engine/Source/Workers/createGeometry.js')).default;
    const batch=suite('createGeometry');
    const invoke=async tasks=>{
      response=undefined;transfers=undefined;
      await worker.onmessage({data:{id:42,parameters:{subTasks:tasks},canTransferArrayBuffer:true}});
      if(response.error) throw Object.assign(new Error(response.error.message??response.error),{name:response.error.name??'Error'});
      return {...response.result,transferCount:transfers.length,unpacked:PrimitivePipeline.unpackCreateGeometryResults(response.result).map(geometryEnc)};
    };
    for(const selected of [[],names.slice(0,10),names.slice(10,21),names.slice(21),['BoxGeometry','BoxGeometry','PlaneOutlineGeometry']]) {
      const args=selected.map((name,i)=>({name,packed:pack(name,description(name,i%5),i%3),offset:i%3,
        mode:i%3===0?'ready':i%3===1?'name':'path'}));
      await batch.add('batch',[args],()=>invoke(args.map(a=>{
        const geometry=a.mode==='ready'?C[a.name].createGeometry(C[a.name].unpack(a.packed,a.offset)):a.packed;
        return {geometry,offset:a.offset,...(a.mode==='name'?{moduleName:'create'+a.name}:a.mode==='path'?{modulePath:'./create'+a.name+'.js'}:{})};
      })));
    }
    await batch.add('undefined',[],()=>invoke([{geometry:undefined},{geometry:C.BoxGeometry.createGeometry(description('BoxGeometry',0))},{geometry:undefined}]));
    await batch.add('conflict',[],()=>invoke([{moduleName:'createBoxGeometry',modulePath:'./createBoxGeometry.js'}]));
    // Stress every component type, CV bounds, normalized attributes and 32-bit index reconstruction.
    for(const vertexCount of [3,65536]) {
      const attributes=new C.GeometryAttributes();
      for(const [name,datatype,Type] of [
        ['position',C.ComponentDatatype.DOUBLE,Float64Array],['normal',C.ComponentDatatype.FLOAT,Float32Array],
        ['i8',C.ComponentDatatype.BYTE,Int8Array],['u8',C.ComponentDatatype.UNSIGNED_BYTE,Uint8Array],
        ['i16',C.ComponentDatatype.SHORT,Int16Array],['u16',C.ComponentDatatype.UNSIGNED_SHORT,Uint16Array],
        ['i32',C.ComponentDatatype.INT,Int32Array],['u32',C.ComponentDatatype.UNSIGNED_INT,Uint32Array],
      ]) {
        const values=new Type(vertexCount);values[0]=1;values[vertexCount-1]=17;
        attributes[name]=new C.GeometryAttribute({componentDatatype:datatype,componentsPerAttribute:1,normalize:name==='u8',values});
      }
      const geometry=new C.Geometry({attributes,indices:C.IndexDatatype.createTypedArray(vertexCount,[0,1,vertexCount-1]),primitiveType:C.PrimitiveType.POINTS,
        geometryType:2,boundingSphere:new C.BoundingSphere(new C.Cartesian3(1,2,3),5),
        boundingSphereCV:new C.BoundingSphere(new C.Cartesian3(4,5,6),7),offsetAttribute:C.GeometryOffsetAttribute.TOP});
      await batch.add('wire',[vertexCount],()=>invoke([{geometry}]));
    }
    suites.push(batch);
    return suites;
  } finally {
    C.ApproximateTerrainHeights._terrainHeights=previousHeights;
    C.ApproximateTerrainHeights._initPromise=previousInit;
    if(previousPost===undefined) delete globalThis.postMessage; else globalThis.postMessage=previousPost;
    if(previousSelf===undefined) delete globalThis.self; else globalThis.self=previousSelf;
  }
}
