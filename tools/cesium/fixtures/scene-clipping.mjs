import {Cartesian3,Matrix4,BoundingSphere,ClippingPlane,ClippingPlaneCollection,Cesium3DTileset,PerspectiveFrustum,Ellipsoid,GeographicProjection,JulianDate} from '@cesium/engine';
import {suite,enc} from './lib.mjs';
import ModelDrawCommand from '@cesium/engine/Source/Scene/Model/ModelDrawCommand.js';
import DrawCommand from '@cesium/engine/Source/Renderer/DrawCommand.js';
import RenderState from '@cesium/engine/Source/Renderer/RenderState.js';
import StencilConstants from '@cesium/engine/Source/Scene/StencilConstants.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
export function stencilSnapshot(rs){const t=rs.stencilTest;return {stencilMask:rs.stencilMask,reference:t.reference,mask:t.mask,frontFunction:t.frontFunction,backFunction:t.backFunction,frontOperation:[t.frontOperation.fail,t.frontOperation.zFail,t.frontOperation.zPass],backOperation:[t.backOperation.fail,t.backOperation.zFail,t.backOperation.zPass],cull:[rs.cull.enabled,rs.cull.face],colorMask:[rs.colorMask.red,rs.colorMask.green,rs.colorMask.blue,rs.colorMask.alpha],polygonOffset:[rs.polygonOffset.enabled,rs.polygonOffset.factor,rs.polygonOffset.units],depthMask:rs.depthMask};}
export function stencilScenario(depth,cull,depthMask){
 const minimum=ContextLimits._minimumAliasedLineWidth,maximum=ContextLimits._maximumAliasedLineWidth;ContextLimits._minimumAliasedLineWidth=1;ContextLimits._maximumAliasedLineWidth=8;
 try{
 const state=RenderState.fromCache({cull:{enabled:cull},depthMask,stencilTest:StencilConstants.setCesium3DTileBit(),stencilMask:StencilConstants.CESIUM_3D_TILE_MASK});
 const model={shadows:1,projectTo2D:false,content:{tileset:{hasMixedContent:true,_backfaceCommands:[]},tile:{_selectionDepth:depth,_finalResolution:false}}};
 const command=new DrawCommand({uniformMap:{},pass:5,modelMatrix:Matrix4.IDENTITY,boundingVolume:new BoundingSphere(Cartesian3.ZERO,1),renderState:state});
 const wrapper=new ModelDrawCommand({command,primitiveRenderResources:{model,runtimePrimitive:{primitive:{material:{doubleSided:false}}},hasSkipLevelOfDetail:true}});const result=[];wrapper.pushCommands({mode:3},result);
 return [stencilSnapshot(state),stencilSnapshot(wrapper._skipLodBackfaceCommand.command.renderState),stencilSnapshot(result[0].renderState)];
 }finally{ContextLimits._minimumAliasedLineWidth=minimum;ContextLimits._maximumAliasedLineWidth=maximum;}
}
export async function mixedDrawScenario(skip,stencil){
 const minimum=ContextLimits._minimumAliasedLineWidth,maximum=ContextLimits._maximumAliasedLineWidth;ContextLimits._minimumAliasedLineWidth=1;ContextLimits._maximumAliasedLineWidth=8;
 const tile=(radius,error,children)=>({boundingVolume:{sphere:[6378137,0,0,radius]},geometricError:error,refine:'REPLACE',content:{uri:`${radius}.glb`},...(children?{children}:{})});const json={asset:{version:'1.1'},geometricError:1000,root:tile(300,500,[tile(100,100,[tile(30,0),tile(20,0)])])};
 const load=Cesium3DTileset.loadJson;Cesium3DTileset.loadJson=async()=>structuredClone(json);let t;
 try{
  t=await Cesium3DTileset.fromUrl('https://example.com/tileset.json',{skipLevelOfDetail:skip,dynamicScreenSpaceError:false,foveatedScreenSpaceError:false,cullWithChildrenBounds:false,cullRequestsWhileMoving:false});const tiles=[t.root,t.root.children[0],...t.root.children[0].children];
  for(const id of [0,2]){const tile=tiles[id];const command=new DrawCommand({uniformMap:{},owner:{id},pass:5,modelMatrix:Matrix4.IDENTITY,boundingVolume:tile.boundingSphere,renderState:RenderState.fromCache({stencilTest:StencilConstants.setCesium3DTileBit(),stencilMask:128})});const model={shadows:1,projectTo2D:false,content:{tileset:t,tile}};const wrapper=new ModelDrawCommand({command,primitiveRenderResources:{model,runtimePrimitive:{primitive:{material:{doubleSided:false}}},hasSkipLevelOfDetail:skip&&stencil}});
   tile._contentState=3;tile._content={ready:true,featuresLength:0,pointsLength:0,trianglesLength:0,geometryByteLength:0,texturesByteLength:0,batchTableByteLength:0,featurePropertiesDirty:false,applyStyle:()=>{},applyDebugSettings:()=>{},update:(_,f)=>wrapper.pushCommands(f,f.commandList),destroy:()=>{},isDestroyed:()=>false};
  }
  const position=new Cartesian3(6379137,0,0),direction=new Cartesian3(-1,0,0),up=Cartesian3.UNIT_Z,frustum=new PerspectiveFrustum({fov:Math.PI/3,aspectRatio:4/3,near:1,far:1e8});
  const frame={camera:{positionWC:position,directionWC:direction,upWC:up,positionCartographic:Ellipsoid.WGS84.cartesianToCartographic(position),frustum,positionWCDeltaMagnitude:0,positionWCDeltaMagnitudeLastFrame:0,timeSinceMoved:1},context:{drawingBufferWidth:800,drawingBufferHeight:600,stencilBuffer:stencil},commandList:[],pixelRatio:1,mode:3,mapProjection:new GeographicProjection(),cullingVolume:frustum.computeCullingVolume(position,direction,up),frameNumber:5,time:new JulianDate(2451545,0),verticalExaggeration:1,verticalExaggerationRelativeHeight:0,afterRender:[]};
  t.prePassesUpdate(frame);t.updateForPass(frame,{pass:0});
  return {mixed:t.hasMixedContent,selected:t._selectedTiles.map(tile=>tiles.indexOf(tile)),commands:frame.commandList.map(c=>({kind:c.stencil===0?'clear':c.renderState.colorMask.red===false?'backface':'front',tile:c.owner?.id??null,reference:c.renderState.stencilTest.reference,mask:c.renderState.stencilMask})),count:t.statistics.numberOfCommands};
 }finally{Cesium3DTileset.loadJson=load;if(t)t.destroy();ContextLimits._minimumAliasedLineWidth=minimum;ContextLimits._maximumAliasedLineWidth=maximum;}
}
export function planeScenario(initial,operations){
 const plane=new ClippingPlane(new Cartesian3(...initial.slice(0,3)),initial[3]);plane.index=3;const changes=[];plane.onChangeCallback=i=>changes.push([i,[plane.normal.x,plane.normal.y,plane.normal.z],plane.distance]);const results=[];
 for(const op of operations){if(op[0]==='distance')plane.distance=op[1];if(op[0]==='normal')plane.normal=new Cartesian3(...op.slice(1));if(op[0]==='component')plane.normal[op[1]]=op[2];if(op[0]==='cloneInto')ClippingPlane.clone(new ClippingPlane(new Cartesian3(...op[1].slice(0,3)),op[1][3]),plane);const clone=ClippingPlane.clone(plane);results.push({plane:[...[plane.normal.x,plane.normal.y,plane.normal.z],plane.distance],clone:[...[clone.normal.x,clone.normal.y,clone.normal.z],clone.distance,clone.index],changes:structuredClone(changes)});}return results;
}
export function collectionScenario(initial,operations,model,transform){
 const planes=initial.map(p=>new ClippingPlane(new Cartesian3(...p.slice(0,3)),p[3]));const collection=new ClippingPlaneCollection({planes,modelMatrix:Matrix4.fromArray(model)});const events=[];const event=(type,p,i)=>events.push([type,i,...[p.normal.x,p.normal.y,p.normal.z],p.distance,p.index]);collection.planeAdded.addEventListener((p,i)=>event('add',p,i));collection.planeRemoved.addEventListener((p,i)=>event('remove',p,i));const result=[];
 for(const op of operations){let returned=null;if(op[0]==='union')collection.unionClippingRegions=op[1];if(op[0]==='enabled')collection.enabled=op[1];if(op[0]==='distance')collection.get(op[1]).distance=op[2];if(op[0]==='component')collection.get(op[1]).normal[op[2]]=op[3];if(op[0]==='add'){const p=op[1];collection.add(new ClippingPlane(new Cartesian3(...p.slice(0,3)),p[3]));}if(op[0]==='remove'){const p=op[1];returned=collection.remove(new ClippingPlane(new Cartesian3(...p.slice(0,3)),p[3]));}if(op[0]==='removeAt')returned=collection.remove(collection.get(op[1]));if(op[0]==='clear')collection.removeAll();
  const intersections=[[-10,0,0,1],[0,0,0,1],[10,0,0,1],[0,-10,0,1],[0,10,0,1],[0,0,10,5]].map(a=>collection.computeIntersectionWithBoundingVolume(new BoundingSphere(new Cartesian3(...a.slice(0,3)),a[3]),Matrix4.fromArray(transform)));
  result.push({returned,state:collection.clippingPlanesState,enabled:collection.enabled,planes:Array.from({length:collection.length},(_,i)=>{const p=collection.get(i);return [...[p.normal.x,p.normal.y,p.normal.z],p.distance,p.index];}),intersections,events:structuredClone(events)});
 }collection.destroy();return result;
}
export default async function clipping(){
 const plane=suite('ClippingPlane'),collection=suite('ClippingPlaneCollection'),stencil=suite('TilesetStencil'),draw=suite('TilesetMixedDraw');
 for(const skip of [false,true])for(const stencil of [false,true]){const args=[skip,stencil];draw.cases.push({f:'draw',a:args,r:enc(await mixedDrawScenario(...args))});}
 for(const depth of [0,1,2,7,8,16])for(const cull of [false,true])for(const depthMask of [false,true])stencil.add('derive',[depth,cull,depthMask],stencilScenario);
 for(const initial of [[1,0,0,0],[0,0,1,-3],[0,.6,.8,100]])plane.add('mutate',[initial,[['distance',initial[3]],['distance',2],['normal',1,0,0],['normal',1,0,0],['component','x',1],['component','z',2],['cloneInto',[0,1,0,5]]]],planeScenario);
 const operations=[['union',false],['union',true],['enabled',false],['enabled',true],['distance',0,5],['add',[0,0,1,0]],['remove',[0,0,1,0]],['remove',[0,0,-1,9]],['removeAt',0],['add',[1,0,0,-4]],['clear'],['add',[0,1,0,2]]];
 for(const initial of [[[1,0,0,0]],[[1,0,0,0],[0,1,0,0]],[[1,0,0,0],[-1,0,0,-2],[0,0,1,0]]])for(const model of [Matrix4.IDENTITY,Matrix4.fromTranslation(new Cartesian3(2,3,4)),Matrix4.fromScale(new Cartesian3(2,.5,3))])for(const transform of [Matrix4.IDENTITY,Matrix4.fromTranslation(new Cartesian3(-5,0,0))])collection.add('mutate',[initial,operations,Array.from(model),Array.from(transform)],collectionScenario);
 return [plane,collection,stencil,draw];
}
