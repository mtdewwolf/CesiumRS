// Deterministic CPU cases from the pinned @cesium/engine 23.0.0.
import {writeFileSync} from 'node:fs';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Matrix3 from '@cesium/engine/Source/Core/Matrix3.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import OrientedBoundingBox from '@cesium/engine/Source/Core/OrientedBoundingBox.js';
import VoxelBoxShape from '@cesium/engine/Source/Scene/VoxelBoxShape.js';
import VoxelShapeType from '@cesium/engine/Source/Scene/VoxelShapeType.js';
import SpatialNode from '@cesium/engine/Source/Scene/SpatialNode.js';
import VoxelCell from '@cesium/engine/Source/Scene/VoxelCell.js';
import VoxelContent from '@cesium/engine/Source/Scene/VoxelContent.js';
import Terrain from '@cesium/engine/Source/Scene/Terrain.js';
import Tileset3DTileContent from '@cesium/engine/Source/Scene/Tileset3DTileContent.js';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Vector3DTileBatch from '@cesium/engine/Source/Scene/Vector3DTileBatch.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import OrderedGroundPrimitiveCollection from '@cesium/engine/Source/Scene/OrderedGroundPrimitiveCollection.js';
const v=a=>new Cartesian3(...a), arr=p=>[p.x,p.y,p.z], obb=b=>[...arr(b.center),...Array.from({length:9},(_,i)=>b.halfAxes[i])];
const output={version:'23.0.0',bounds:[],boxes:[],temporal:[],cells:[]};
for(const type of Object.keys(VoxelShapeType).filter(k=>typeof VoxelShapeType[k]==='string')) output.bounds.push({type,min:arr(VoxelShapeType.getMinBounds(type)),max:arr(VoxelShapeType.getMaxBounds(type))});
for(let i=0;i<24;i++) {
 const shape=new VoxelBoxShape(), min=[-1,-.75,-.5],max=[1,.5,1];
 const scale=[1+i*.25,2,3],translation=[i,-2,4];
 if(i%3===1) scale[0]*=-1;
 const model=Matrix4.fromRotationTranslation(Matrix3.fromRotationZ(i*.17),v(translation));Matrix4.multiplyByScale(model,v(scale),model);
 const visible=shape.update(model,v(min),v(max));const level=i%4,coords=[0,0,0];
 const dims=[4,2,2],uv=[.25,.5,0];
 output.boxes.push({model:Array.from({length:16},(_,i)=>model[i]),min,max,scale,translation,visible,level,coords,dims,uv,box:obb(shape.orientedBoundingBox),tile:obb(shape.computeOrientedBoundingBoxForTile(level,...coords,new OrientedBoundingBox())),sample:obb(shape.computeOrientedBoundingBoxForSample({level,x:0,y:0,z:0},v(dims),v(uv),new OrientedBoundingBox())),uvResult:arr(shape.convertLocalToShapeUvSpace(v([.2,.1,-.1]),new Cartesian3()))});
}
const shape=new VoxelBoxShape();shape.update(Matrix4.IDENTITY,v([-1,-1,-1]),v([1,1,1]));
for(const location of [-2,0,.2,1,1.8,2.5,4,10]) {
 const parent=new SpatialNode(0,0,0,0,undefined,shape,v([2,2,2]));const node=new SpatialNode(1,1,0,1,parent,shape,v([2,2,2]));
 parent.renderableKeyframeNodes=[0,2,4].map(keyframe=>({keyframe,spatialNode:parent}));node.renderableKeyframeNodes=[1,3].map(keyframe=>({keyframe,spatialNode:node}));
 node.computeSurroundingRenderableKeyframeNodes(location);node.computeScreenSpaceError(v([3,4,5]),2);
 output.temporal.push({location,prev:[node.renderableKeyframeNodePrevious.spatialNode.level,node.renderableKeyframeNodePrevious.keyframe],next:[node.renderableKeyframeNodeNext.spatialNode.level,node.renderableKeyframeNodeNext.keyframe],lerp:node.renderableKeyframeNodeLerp,error:node.screenSpaceError});
}
const data=[new Uint16Array(Array.from({length:64},(_,i)=>i*3)),new Float32Array(Array.from({length:192},(_,i)=>i*.25))];
for(const sample of [0,1,5,21,63,70]) {
 const content=VoxelContent.fromMetadataArray(data);const primitive={provider:{names:['density','velocity'],types:['SCALAR','VEC3']},_paddingBefore:v([1,1,1]),dimensions:v([2,2,2]),_shape:shape};
 const cell=VoxelCell.fromKeyframeNode(primitive,7,sample,{spatialNode:{level:0,x:0,y:0,z:0,dimensions:v([4,4,4])},content});
 output.cells.push({sample,density:Array.from(cell.getProperty('density')),velocity:Array.from(cell.getProperty('velocity')),box:obb(cell.orientedBoundingBox)});
}
const content=VoxelContent.fromMetadataArray([]),frame={afterRender:[]};output.content=[content.ready];content.update({},frame);output.content.push(content.ready,frame.afterRender.length);content.update({},frame);output.content.push(frame.afterRender.length);content.destroy();output.content.push(content.isDestroyed());
const provider={id:42},terrain=new Terrain(Promise.resolve(provider));output.terrain=[terrain.ready];terrain.readyEvent.addEventListener(p=>output.terrain.push(p.id));await Promise.resolve();output.terrain.push(terrain.ready);
let loaded=0;const tiles=Tileset3DTileContent.fromJson({loadTileset(r,j,t){loaded++;}}, {},new Resource('https://example.test/tiles.json?q=1'),{});output.tileset={loaded,ready:tiles.ready,url:tiles.url,features:tiles.featuresLength,has:tiles.hasProperty(0,'x')};
const batch=new Vector3DTileBatch({offset:3,count:12,color:new Color(.1,.2,.3,.4),batchIds:[2,8]});output.batch={offset:batch.offset,count:batch.count,color:[batch.color.red,batch.color.green,batch.color.blue,batch.color.alpha],ids:batch.batchIds};
const ordered=new OrderedGroundPrimitiveCollection(),events=[];const prim=id=>({update(){events.push(id)},destroy(){events.push(-id)}});const a=prim(1),b=prim(2),c=prim(3);ordered.add(a,5);ordered.add(b,-1);ordered.add(c,5);ordered.update({});ordered.set(a,-1);ordered.update({});ordered.remove(b);ordered.show=false;ordered.update({});ordered.removeAll();output.ordered=events;
// Minimal DOM adapter lets us exercise the source sampling implementation.
let now=0;Object.defineProperty(globalThis,'performance',{value:{now:()=>now},configurable:true});
globalThis.Element=class {};const element=()=>({appendChild(){},className:''});globalThis.document={createElement:element,createTextNode:()=>({nodeValue:''})};
const {default:PerformanceDisplay}=await import('@cesium/engine/Source/Scene/PerformanceDisplay.js');
const container=new Element();container.appendChild=()=>{};const display=new PerformanceDisplay({container});
output.performance=[];
for(const [time,rendered] of [[0,true],[200,true],[201,true],[1000,true],[1001,true],[1202,false],[2400,false],[2601,true]]) {now=time;display.update(rendered);output.performance.push({time,rendered,fps:display._fpsText.nodeValue,ms:display._msText.nodeValue});}
writeFileSync(new URL('../../tests/fixtures/scene/MzCpu.json',import.meta.url),JSON.stringify({version:output.version,cases:Object.entries(output).filter(([name])=>name!=='version').map(([name,value])=>({name,value}))},null,2)+'\n');
console.log('Wrote MzCpu.json');
