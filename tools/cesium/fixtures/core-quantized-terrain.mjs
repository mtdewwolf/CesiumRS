import { AttributeCompression, BoundingSphere, Cartesian2, Cartesian3, Ellipsoid, GeographicTilingScheme, Matrix3, OrientedBoundingBox, QuantizedMeshTerrainData, Rectangle, TaskProcessor, TerrainEncoding, WebMercatorTilingScheme } from "@cesium/engine";
import { enc, generators, makeRng } from "./lib.mjs";
const c3=a=>new Cartesian3(...a);
const scheme=merc=>merc?new WebMercatorTilingScheme():new GeographicTilingScheme();
const box=b=>b?new OrientedBoundingBox(c3(b[0]),Matrix3.fromArray(b[1])):undefined;
const create=o=>new QuantizedMeshTerrainData({quantizedVertices:new Uint16Array(o.vertices),indices:new ({Uint16Array,Uint32Array}[o.indexType])(o.indices),encodedNormals:o.normals?new Uint8Array(o.normals):undefined,minimumHeight:o.minimum,maximumHeight:o.maximum,boundingSphere:new BoundingSphere(c3(o.sphere.slice(0,3)),o.sphere[3]),orientedBoundingBox:box(o.box),horizonOcclusionPoint:c3(o.horizon),westIndices:o.edges[0],southIndices:o.edges[1],eastIndices:o.edges[2],northIndices:o.edges[3],westSkirtHeight:o.skirts[0],southSkirtHeight:o.skirts[1],eastSkirtHeight:o.skirts[2],northSkirtHeight:o.skirts[3],childTileMask:o.mask,waterMask:o.water?new Uint8Array(o.water):undefined,credits:o.credits,createdByUpsampling:o.upsampled});
const encoding=e=>[e.quantization,e.minimumHeight,e.maximumHeight,e.stride,e.hasVertexNormals,e.hasWebMercatorT,e.hasGeodeticSurfaceNormals,e.exaggeration,e.exaggerationRelativeHeight,e.center,e.matrix??null,e.toScaledENU??null,e.fromScaledENU??null];
const edges=e=>[e.westIndicesSouthToNorth,e.southIndicesEastToWest,e.eastIndicesNorthToSouth,e.northIndicesWestToEast];
const raw=d=>[d._quantizedVertices??null,d._indices?.constructor.name??null,d._indices??null,d._encodedNormals??null,d._westIndices?[d._westIndices,d._southIndices,d._eastIndices,d._northIndices]:null,d._minimumHeight,d._maximumHeight,d._boundingSphere,d._orientedBoundingBox?[d._orientedBoundingBox.center,d._orientedBoundingBox.halfAxes]:null,d._horizonOcclusionPoint,d._westSkirtHeight,d._southSkirtHeight,d._eastSkirtHeight,d._northSkirtHeight,d.credits??null,d.waterMask??null,d.childTileMask,d.wasCreatedByUpsampling(),d.canUpsample];
const mesh=m=>[Cartesian3.clone(m.center),m.vertices,m.indices.constructor.name,m.indices,m.indexCountWithoutSkirts,m.vertexCountWithoutSkirts,m.minimumHeight,m.maximumHeight,m.rectangle,m.boundingSphere3D,m.occludeePointInScaledSpace,m.stride,m.orientedBoundingBox?[m.orientedBoundingBox.center,m.orientedBoundingBox.halfAxes]:null,edges(m),encoding(m.encoding)];
const sample=(d,r,points)=>points.map(([x,y])=>d.interpolateHeight(r,r.west+r.width*x,r.south+r.height*y)??null);
const suite=module=>({module,cases:[],async add(f,args,fn){let r;try{r=enc(await fn(...args));}catch(e){r={throws:String(e.message??e),kind:e.name??"Error"};}this.cases.push({f,a:args.map(enc),r});}});
const meshResult=(r,vertexCount)=>{
  const count=vertexCount+r.westIndicesSouthToNorth.length+r.southIndicesEastToWest.length+r.eastIndicesNorthToSouth.length+r.northIndicesWestToEast.length;
  const indices=count>=65536?new Uint32Array(r.indices):new Uint16Array(r.indices);
  return [new Float32Array(r.vertices),indices.constructor.name,indices,edges(r),r.vertexStride,Cartesian3.clone(r.center),r.minimumHeight,r.maximumHeight,Cartesian3.clone(r.occludeePointInScaledSpace)??null,encoding(TerrainEncoding.clone(r.encoding)),r.indexCountWithoutSkirts];
};
const upsampleResult=r=>{
  const vertices=new Uint16Array(r.vertices),indices=vertices.length/3>=65536?new Uint32Array(r.indices):new Uint16Array(r.indices);
  return [vertices,indices.constructor.name,indices,r.encodedNormals?new Uint8Array(r.encodedNormals):null,r.minimumHeight,r.maximumHeight,[r.westIndices,r.southIndices,r.eastIndices,r.northIndices],BoundingSphere.clone(r.boundingSphere),[Cartesian3.clone(r.orientedBoundingBox.center),Matrix3.clone(r.orientedBoundingBox.halfAxes)],Cartesian3.clone(r.horizonOcclusionPoint)??null];
};

export default async function coreQuantizedTerrain(){
  const oldSelf=globalThis.self,oldPost=globalThis.postMessage,previous=TaskProcessor.prototype.scheduleTask;
  const workers={};
  for(const name of ["createVerticesFromQuantizedTerrainMesh","upsampleQuantizedTerrainMesh"]){globalThis.self={};workers[name]=(await import(`@cesium/engine/Source/Workers/${name}.js`)).default;}
  globalThis.self=oldSelf;
  const process=async(name,parameters)=>{
    let response;globalThis.self=workers[name];globalThis.postMessage=r=>{response=r;};
    try{await workers[name].onmessage({data:{id:0,parameters,canTransferArrayBuffer:false}});if(response.error){const e=new Error(response.error.message??response.error);e.name=response.error.name??"Error";throw e;}return structuredClone(response.result);}
    finally{globalThis.self=oldSelf;globalThis.postMessage=oldPost;}
  };
  TaskProcessor.prototype.scheduleTask=function(parameters){return process(this._workerPath,parameters);};
  const s=suite("QuantizedMeshTerrainData"),workerSuite=suite("createVerticesFromQuantizedTerrainMesh"),upsampleSuite=suite("upsampleQuantizedTerrainMesh");
  const g=generators(makeRng(2200));
  try{
    for(let i=0;i<56;i++){
      const merc=i%2===0,ts=scheme(merc),tile=[i%2,i%2,[2,7,12,16,19][i%5]],r=ts.tileXYToRectangle(...tile);
      const width=2+i%4,height=2+(i>>1)%4,count=width*height;
      const u=[],v=[],h=[],indices=[],west=[],south=[],east=[],north=[];
      for(let y=0;y<height;y++)for(let x=0;x<width;x++){
        const j=y*width+x;u.push(Math.round(x/(width-1)*32767));v.push(Math.round(y/(height-1)*32767));h.push(j===0?0:j===count-1?32767:Math.floor(g.rng()*32768));
        if(x===0)west.push(j);if(x===width-1)east.push(j);if(y===0)south.push(j);if(y===height-1)north.push(j);
        if(x<width-1&&y<height-1)indices.push(j,j+1,j+width,j+1,j+width+1,j+width);
      }
      const shuffle=a=>a.map(x=>[g.rng(),x]).sort((a,b)=>a[0]-b[0]).map(x=>x[1]);
      const minimum=i%4===0?50:-100,maximum=i%4===0?50:900;
      const sphere=Ellipsoid.WGS84.cartographicToCartesian(Rectangle.center(r));
      const o={vertices:[...u,...v,...h],indices,indexType:i%3?"Uint16Array":"Uint32Array",minimum,maximum,sphere:[sphere.x,sphere.y,sphere.z,100000],horizon:[1,2,3],box:i%3?null:[[sphere.x,sphere.y,sphere.z],Array.from(Matrix3.IDENTITY)],edges:[shuffle(west),shuffle(south),shuffle(east),shuffle(north)],skirts:[10,20,30,40],normals:i%2?null:Array.from({length:count},()=>{const n=AttributeCompression.octEncode(g.unit3(),new Cartesian2());return[n.x,n.y];}).flat(),mask:i%16,credits:["terrain-a","terrain-b"],water:i%2?null:[0,255],upsampled:i%9===0};
      if(i===54)o.edges=[[],[],[],[]];
      if(i===55)o.edges[0]=[];
      const points=[[-1,-1],[0,0],[1,1],[.5,.5],[1/32767,1/32767],[2,2],...Array.from({length:10},()=>[g.rng(),g.rng()])];
      await s.add("lifecycle",[o,merc,tile,points,i%3===0?2.5:1,30],async(o,merc,tile,points,exaggeration,relative)=>{
        const d=create(o),ts=scheme(merc),r=ts.tileXYToRectangle(...tile);
        const initial=[raw(d),sample(d,r,points),Array.from({length:9},(_,j)=>d.isChildAvailable(tile[0],tile[1],tile[0]*2+j%3,tile[1]*2+Math.floor(j/3))),d.upsample(ts,...tile,tile[0]*2,tile[1]*2,tile[2]+1)??null];
        const m=await d.createMesh({tilingScheme:ts,x:tile[0],y:tile[1],level:tile[2],exaggeration,exaggerationRelativeHeight:relative});
        const children=[];
        for(let y=0;y<2;y++)for(let x=0;x<2;x++){
          const child=await d.upsample(ts,...tile,tile[0]*2+x,tile[1]*2+y,tile[2]+1),r=ts.tileXYToRectangle(tile[0]*2+x,tile[1]*2+y,tile[2]+1);
          children.push([raw(child),sample(child,r,points)]);
        }
        return [initial,mesh(m),raw(d),sample(d,r,points),children];
      });
      if(i<16){
        const rect=i===0?Rectangle.fromDegrees(179,-10,-179,-8):r;
        await workerSuite.add("create",[o,[rect.west,rect.south,rect.east,rect.north],i%3!==0,i%4===0?3:1],async(o,r,merc,exaggeration)=>{
          const result=await process("createVerticesFromQuantizedTerrainMesh",{quantizedVertices:new Uint16Array(o.vertices),indices:new Uint32Array(o.indices),octEncodedNormals:o.normals?new Uint8Array(o.normals):undefined,minimumHeight:o.minimum,maximumHeight:o.maximum,westIndices:o.edges[0],southIndices:o.edges[1],eastIndices:o.edges[2],northIndices:o.edges[3],westSkirtHeight:o.skirts[0],southSkirtHeight:o.skirts[1],eastSkirtHeight:o.skirts[2],northSkirtHeight:o.skirts[3],rectangle:new Rectangle(...r),relativeToCenter:c3(o.sphere.slice(0,3)),ellipsoid:Ellipsoid.WGS84,includeWebMercatorT:merc,exaggeration,exaggerationRelativeHeight:30});
          return meshResult(result,o.vertices.length/3);
        });
        await upsampleSuite.add("upsample",[o,merc,tile,i%2===0,i%3===0],async(o,merc,tile,east,north)=>{
          const d=create(o),ts=scheme(merc),m=await d.createMesh({tilingScheme:ts,x:tile[0],y:tile[1],level:tile[2]});
          const r=ts.tileXYToRectangle(tile[0]*2+(east?1:0),tile[1]*2+(north?0:1),tile[2]+1);
          const result=await process("upsampleQuantizedTerrainMesh",{vertices:m.vertices,indices:m.indices,vertexCountWithoutSkirts:m.vertexCountWithoutSkirts,indexCountWithoutSkirts:m.indexCountWithoutSkirts,encoding:m.encoding,minimumHeight:o.minimum,maximumHeight:o.maximum,isEastChild:east,isNorthChild:north,childRectangle:r,ellipsoid:Ellipsoid.WGS84});
          return upsampleResult(result);
        });
      }
    }
    // Surface indices fit Uint16, but skirt vertices push the total over the
    // Uint32 threshold. Keep only the relevant output slices in this case.
    const count=65535,vertices=[...Array(count).fill(16383),...Array(count).fill(16383),...Array(count).fill(16383)];
    vertices[0]=0;vertices[1]=32767;vertices[2]=0;vertices[3]=32767;
    vertices[count]=0;vertices[count+1]=0;vertices[count+2]=32767;vertices[count+3]=32767;
    const rect=Rectangle.fromDegrees(-1,.5,-.99,.51),center=Ellipsoid.WGS84.cartographicToCartesian(Rectangle.center(rect));
    const large={vertices,indices:[0,1,2,1,3,2],indexType:"Uint16Array",minimum:-10,maximum:1000,sphere:[center.x,center.y,center.z,1000],horizon:[1,2,3],edges:[[0,2],[0,1],[1,3],[2,3]],skirts:[10,20,30,40],mask:15,upsampled:false};
    await workerSuite.add("largeCreate",[large,[rect.west,rect.south,rect.east,rect.north]],async(o,r)=>{
      const result=await process("createVerticesFromQuantizedTerrainMesh",{quantizedVertices:new Uint16Array(o.vertices),indices:new Uint16Array(o.indices),minimumHeight:o.minimum,maximumHeight:o.maximum,westIndices:o.edges[0],southIndices:o.edges[1],eastIndices:o.edges[2],northIndices:o.edges[3],westSkirtHeight:10,southSkirtHeight:20,eastSkirtHeight:30,northSkirtHeight:40,rectangle:new Rectangle(...r),relativeToCenter:c3(o.sphere.slice(0,3)),ellipsoid:Ellipsoid.WGS84,includeWebMercatorT:true,exaggeration:1,exaggerationRelativeHeight:0});
      const vertices=new Float32Array(result.vertices),indices=new Uint32Array(result.indices);
      return [vertices.length,Array.from(vertices.slice(0,24)),Array.from(vertices.slice(-24)),indices.constructor.name,indices,encoding(TerrainEncoding.clone(result.encoding))];
    });
    return [s,workerSuite,upsampleSuite];
  }finally{TaskProcessor.prototype.scheduleTask=previous;globalThis.self=oldSelf;globalThis.postMessage=oldPost;}
}
