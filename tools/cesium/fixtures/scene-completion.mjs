import FrameState from '@cesium/engine/Source/Scene/FrameState.js';
import View from '@cesium/engine/Source/Scene/View.js';
import Implicit3DTileContent from '@cesium/engine/Source/Scene/Implicit3DTileContent.js';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import DerivedCommand from '@cesium/engine/Source/Scene/DerivedCommand.js';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import DrawCommand from '@cesium/engine/Source/Renderer/DrawCommand.js';
import RenderState from '@cesium/engine/Source/Renderer/RenderState.js';
import MetadataClassProperty from '@cesium/engine/Source/Scene/MetadataClassProperty.js';
import Camera from '@cesium/engine/Source/Scene/Camera.js';
import {Scene,Cartesian3,BoundingSphere,PerspectiveFrustum,OrthographicOffCenterFrustum,GeographicProjection,Ellipsoid,Cesium3DTileset,JulianDate,Matrix4} from '@cesium/engine';
import {suite,enc,makeRng} from './lib.mjs';

function derive(input,operations,metadata) {
  const context={shaderCache:{getDerivedShaderProgram:()=>undefined,createDerivedShaderProgram:(program,key,options)=>({...options,id:2,_attributeLocations:options.attributeLocations})}};
  const scene={_depthOnlyRenderStateCache:{},picking:{pickRenderStateCache:{}}};
  let command=new DrawCommand({shaderProgram:{id:1,vertexShaderSource:new ShaderSource(input.vertex),fragmentShaderSource:new ShaderSource(input.fragment),_attributeLocations:{}},renderState:RenderState.fromCache({depthMask:false,blending:{enabled:true}}),pickId:'u_pickColor'});
  for(const op of operations) {
    if(op==='log')command=DerivedCommand.createLogDepthCommand(command,context).command;
    else if(op==='hdr')command=DerivedCommand.createHdrCommand(command,context).command;
    else if(op==='depth')command=DerivedCommand.createDepthOnlyDerivedCommand(scene,command,context).depthOnlyCommand;
    else if(op==='pick')command=DerivedCommand.createPickDerivedCommand(scene,command,context).pickCommand;
    else if(op==='metadata'){
      const classProperty=MetadataClassProperty.fromJson({id:'p',property:metadata.property});
      const named=v=>Array.isArray(v)?Object.fromEntries(v.map((n,i)=>[['x','y','z','w'][i],n])):v;
      command.pickedMetadataInfo={schemaId:'s',className:'c',propertyName:'p',classProperty,metadataProperty:{hasValueTransform:!!metadata.transform,offset:named(metadata.transform?.[0]),scale:named(metadata.transform?.[1])}};
      command=DerivedCommand.createPickMetadataDerivedCommand(scene,command,context).pickMetadataCommand;
    }
  }
  const shader=s=>({sources:s.sources,defines:s.defines,includeBuiltIns:s.includeBuiltIns,pickColorQualifier:s.pickColorQualifier??null});
  return {vertex:shader(command.shaderProgram.vertexShaderSource),fragment:shader(command.shaderProgram.fragmentShaderSource),state:{depthMask:command.renderState.depthMask,colorMask:command.renderState.colorMask,blend:command.renderState.blending.enabled}};
}
function cameraEvents(initial,steps,wait) {
  let timestamp=0;const previous=globalThis.performance;globalThis.performance={now:()=>timestamp};
  try{
    const scene={mapProjection:new GeographicProjection(),drawingBufferWidth:800,drawingBufferHeight:600,pixelRatio:1,canvas:{clientWidth:800,clientHeight:600},ellipsoid:Ellipsoid.WGS84,cameraEventWaitTime:wait};
    const camera=new Camera(scene);camera.position=new Cartesian3(...initial);
    const view={camera,_cameraClone:Camera.clone(camera),_cameraStartFired:false};let events=[];
    camera.moveStart.addEventListener(()=>events.push('start'));camera.moveEnd.addEventListener(()=>events.push('end'));
    return steps.map(step=>{timestamp=step.time;if(step.position)camera.position=new Cartesian3(...step.position);if(step.fov)camera.frustum.fov=step.fov;events=[];const changed=View.prototype.checkForCameraUpdates.call(view,scene);return {changed,events};});
  }finally{globalThis.performance=previous;}
}

function fit(options,commands) {
  const mode=options.mode??3;
  const frustum=mode===2?new OrthographicOffCenterFrustum({left:-1000,right:1000,top:1000,bottom:-1000,near:options.near??1,far:options.far??1000}):new PerspectiveFrustum({fov:Math.PI/3,aspectRatio:4/3,near:options.near??1,far:options.far??1000});
  const position=new Cartesian3(...(options.position??[0,0,100]));const direction=new Cartesian3(0,0,-1),up=new Cartesian3(0,1,0);
  const frame={camera:{position,positionWC:position,directionWC:direction,frustum},mode,useLogDepth:options.log??false,shadowState:{shadowsEnabled:true},
    cullingVolume:frustum.computeCullingVolume(position,direction,up),frustumSplits:[],commandList:commands.map((c,i)=>({id:i,pass:c.pass??8,boundingVolume:c.sphere?new BoundingSphere(Cartesian3.fromArray(c.sphere),c.sphere[3]):undefined,cull:c.cull??true,occlude:true,receiveShadows:c.shadows??false,executeInClosestFrustum:c.closest??false}))};
  const scene={frameState:frame,mode,farToNearRatio:options.ratio??10,logarithmicDepthFarToNearRatio:options.logRatio??1e9,nearToFarDistance2D:options.step??100,debugShowFrustums:true,_computeCommandList:[],_overlayCommandList:[],updateDerivedCommands:()=>{},isVisible:Scene.prototype.isVisible};
  const view={frustumCommandsList:[],_commandExtents:[]};View.prototype.createPotentiallyVisibleSet.call(view,scene);
  return {frustums:view.frustumCommandsList.map(f=>({near:f.near,far:f.far,commands:f.commands.map((a,p)=>a.slice(0,f.indices[p]).map(c=>[c.id,c.debugOverlappingFrustums]))})),splits:frame.frustumSplits,
    compute:scene._computeCommandList.map(c=>c.id),overlay:scene._overlayCommandList.map(c=>c.id),statistics:view.debugFrustumStatistics,
    shadows:{near:frame.shadowState.nearPlane,far:frame.shadowState.farPlane,closest:frame.shadowState.closestObjectSize}};
}
async function traverse(json,options,ready,config,pass) {
  const load=Cesium3DTileset.loadJson;Cesium3DTileset.loadJson=async()=>structuredClone(json);let t;
  try{t=await Cesium3DTileset.fromUrl('https://example.com/tileset.json',options);}finally{Cesium3DTileset.loadJson=load;}
  const tiles=[],queue=[t.root];while(queue.length){const tile=queue.shift();tiles.push(tile);queue.push(...tile.children);}
  const ids=new Map(tiles.map((t,i)=>[t,i]));
  for(const id of ready)if(!tiles[id].hasEmptyContent){tiles[id]._contentState=3;tiles[id]._content={featurePropertiesDirty:false,featuresLength:0,pointsLength:0,trianglesLength:0,geometryByteLength:0,texturesByteLength:0,batchTableByteLength:0,destroy:()=>{},isDestroyed:()=>false};}
  const frustum=new PerspectiveFrustum({fov:config.fov??Math.PI/3,aspectRatio:4/3,near:1,far:1e8});
  const position=new Cartesian3(...config.position);const direction=Cartesian3.normalize(new Cartesian3(...(config.direction??[-1,0,0])),new Cartesian3());
  const up=new Cartesian3(0,0,1);const camera={position,positionWC:position,directionWC:direction,upWC:up,positionCartographic:Ellipsoid.WGS84.cartesianToCartographic(position),frustum,positionWCDeltaMagnitude:0,positionWCDeltaMagnitudeLastFrame:0,timeSinceMoved:config.timeSinceMoved??1};
  const frame={camera,context:{drawingBufferWidth:800,drawingBufferHeight:600},pixelRatio:config.pixelRatio??1,mode:3,mapProjection:new GeographicProjection(),
    cullingVolume:frustum.computeCullingVolume(position,direction,up),frameNumber:5,time:new JulianDate(2451545,0),verticalExaggeration:1,verticalExaggerationRelativeHeight:0};
  t._pass=pass;t._updatedVisibilityFrame=1;t.getTraversal({pass}).selectTiles(t,frame);
  const result={selected:t._selectedTiles.map(t=>ids.get(t)),requested:t._requestedTiles.map(t=>({id:ids.get(t),priority:t._priority})).sort((a,b)=>a.id-b.id),empty:t._emptyTiles.map(t=>ids.get(t)),visited:t.statistics.visited,mixed:t.hasMixedContent,
    selection:t._selectedTiles.map(t=>[ids.get(t),t._selectionDepth,t._finalResolution]),styled:t._selectedTilesToStyle.map(t=>ids.get(t))};t.destroy();return result;
}
async function expand(json,subtree,buffers) {
  const load=Cesium3DTileset.loadJson,fetch=Resource.prototype.fetchArrayBuffer;
  Cesium3DTileset.loadJson=async()=>structuredClone(json);
  Resource.prototype.fetchArrayBuffer=function(){const name=this.url.split('/').at(-1);if(!buffers[name])return Promise.reject(new Error(`missing ${this.url}`));return Promise.resolve(new Uint8Array(buffers[name]).buffer);};
  let t;
  try{
    t=await Cesium3DTileset.fromUrl('https://example.com/tileset.json');
    const content=await Implicit3DTileContent.fromSubtreeJson(t,t.root,t.root._contentResource,structuredClone(subtree));
    t.root._content=content;
    const tiles=[],queue=[t.root];while(queue.length){const tile=queue.shift(),c=tile.implicitCoordinates;tiles.push({depth:tile._depth,error:tile.geometricError,bounds:tile._header.boundingVolume,sphere:enc(tile.boundingSphere),children:tile.children.length,placeholder:!!tile.implicitTileset,coordinates:c?[c.level,c.x,c.y,c.z??null]:null,contents:(tile._header.contents??(tile._header.content?[tile._header.content]:[])).map(c=>c.uri??c.url)});queue.push(...tile.children);}
    const result={total:t.statistics.numberOfTilesTotal,tiles};t.destroy();return result;
  }finally{Cesium3DTileset.loadJson=load;Resource.prototype.fetchArrayBuffer=fetch;}
}
async function dynamicError(json,position,direction,matrix) {
  const load=Cesium3DTileset.loadJson;Cesium3DTileset.loadJson=async()=>structuredClone(json);let t;
  try{t=await Cesium3DTileset.fromUrl('https://example.com/tileset.json');}finally{Cesium3DTileset.loadJson=load;}
  t.modelMatrix=Matrix4.fromArray(matrix);t.root.updateTransform(t.modelMatrix);
  const camera={positionWC:new Cartesian3(...position),directionWC:new Cartesian3(...direction),positionCartographic:Ellipsoid.WGS84.cartesianToCartographic(new Cartesian3(...position))};
  t.prePassesUpdate({camera,mapProjection:new GeographicProjection(),time:new JulianDate(2451545,0),frameNumber:1,newFrame:true,context:{}});
  const density=t._dynamicScreenSpaceErrorComputedDensity;t.destroy();return density;
}

export default async function sceneCompletion() {
  const frame=suite('FrameState'),view=suite('SceneView'),traversal=suite('TilesetTraversal'),expansion=suite('TilesetExpansion'),derived=suite('DerivedCommand'),movement=suite('SceneCameraEvents'),dynamic=suite('TilesetDynamicError');
  for(const bounds of [{box:[0,0,10,20,0,0,0,20,0,0,0,10]},{sphere:[0,0,10,10]},{region:[-.1,-.1,.1,.1,0,100]},{sphere:[6378137,0,0,100]}])for(const content of [null,{sphere:[0,0,5,5]}])for(const height of [2,20,100]){
    const json={asset:{version:'1.1'},geometricError:100,root:{boundingVolume:bounds,geometricError:0,content:{uri:'a.glb',...(content?{boundingVolume:content}:{})}}};
    const matrix=Array.from(Matrix4.fromScale(new Cartesian3(1,1,2)));const position=bounds.region?[6378137+height,0,0]:[0,0,height];const args=[json,position,[1,0,0],matrix];dynamic.cases.push({f:'density',a:enc(args),r:enc(await dynamicError(...args))});
  }
  for(const position of [[0,0,0],[6378137,0,0],[1e15,0,0]])for(const wait of [0,50,500]){
    const x=position[0];const steps=[{time:0},{time:10,position:[x+1e-8,0,0]},{time:20,position:[x+10,0,0]},{time:20+wait},{time:21+wait},{time:1000,fov:.7},{time:1600}];movement.add('events',[position,steps,wait],cameraEvents);
  }
  const inputs=[{vertex:{sources:['void main(){gl_Position=vec4(1.0);}']},fragment:{sources:['void main(){out_FragColor=vec4(1.0);}']}},
    {vertex:{sources:['void main(){gl_Position=vec4(1.0); czm_vertexLogDepth();}']},fragment:{sources:['void main(){discard; out_FragData_0=vec4(1.0);}']}},
    {vertex:{sources:['void main(void){gl_Position=vec4(1.0);}']},fragment:{sources:['void main(){gl_FragDepth=.5;}']}},
    {vertex:{sources:['void main(){gl_Position=vec4(1.0);}']},fragment:{defines:['LOG_DEPTH_READ_ONLY'],sources:['void main(){out_FragColor=vec4(1.0);}']}},
    {vertex:{sources:['void main(){gl_Position=vec4(1.0);}']},fragment:{defines:['LOG_DEPTH_WRITE'],sources:['void main(){out_FragColor=vec4(1.0);}']}}];
  for(const input of inputs)for(const operations of [['depth'],['pick'],['log'],['hdr'],['log','depth'],['log','pick'],['log','hdr']])derived.add('derive',[input,operations,null],derive);
  for(const type of ['SCALAR','VEC2','VEC4'])for(const normalized of [false,true])for(const transformed of [false,true]){
    const count={SCALAR:1,VEC2:2,VEC4:4}[type];
    const input={vertex:{sources:['void main(){gl_Position=vec4(1.0);}']},fragment:{sources:['void main(){out_FragColor=vec4(1.0);}'],defines:['METADATA_PICKING_VALUE_TYPE float','METADATA_PICKING_VALUE_STRING 0.0',...['X','Y','Z','W'].map(c=>`METADATA_PICKING_VALUE_COMPONENT_${c} 0.0`)]}};
    const metadata={property:{type,componentType:'UINT16',normalized},transform:transformed?(count===1?[2,3]:[Array(count).fill(2),Array(count).fill(3)]):null};derived.add('derive',[input,['metadata'],metadata],derive);
  }
  for(const services of [[],[{name:'context'},{name:'credits'},{name:'jobs'}]])frame.add('defaults',[services],services=>{
    const f=new FrameState(...services);
    // Rust requires initialized camera, projection and time in its owned
    // UniformFrame; all remaining source fields retain these defaults.
    const skip=new Set(['camera','mapProjection','time','cullingVolume']);
    return Object.fromEntries(Object.entries(f).filter(([key])=>!skip.has(key)).map(([key,value])=>[key,enc(value)]));
  });
  for(const mode of [3,1,2])for(const log of [false,true])for(const commands of [[],[{}],[{pass:1},{pass:12}],[{sphere:[0,0,0,1]}],[{sphere:[0,0,100,1]}],[{sphere:[10000,0,0,1]}],
    [{sphere:[0,0,0,50],closest:true},{sphere:[0,0,-400,200],shadows:true}],[{sphere:[0,0,10000,2],cull:false},{sphere:[0,0,99,5],pass:2,shadows:true}]])view.add('fit',[{mode,log},commands],fit);
  const rng=makeRng(0x71ee);
  for(let n=0;n<48;n++) {
    const depth=1+n%4;let count=0;
    const node=(level,x,y,size)=>{count++;const leaf=level===depth;const box=n%2===0;
      const boundingVolume=box?{box:[x,y,0,size,0,0,0,size,0,0,0,size]}:{sphere:[x,y,0,size]};
      const tile={boundingVolume,geometricError:leaf?0:size*.5,refine:n%5===0?'ADD':'REPLACE',content:{uri:`${count}.glb`}};
      if(!leaf){tile.children=[node(level+1,x-10,y-size*(n%4===0?.3:.6),size*.4),node(level+1,x+10,y+size*(n%4===0?.3:.6),size*.4)];if(n%7===0)delete tile.content;}
      return tile;};
    const json={asset:{version:'1.1'},geometricError:1000,root:node(0,6378137,0,200)};
    const options={dynamicScreenSpaceError:false,foveatedScreenSpaceError:n%3===0,cullWithChildrenBounds:n%4===0,cullRequestsWhileMoving:false,skipLevelOfDetail:n%2===0,
      baseScreenSpaceError:[16,100,1024][n%3],skipScreenSpaceErrorFactor:2,skipLevels:n%3,immediatelyLoadDesiredLevelOfDetail:n%6===0,loadSiblings:n%3===1,preferLeaves:n%2===1,progressiveResolutionHeightFraction:[0,.3,.5][n%3],maximumScreenSpaceError:16};
    const ready=Array.from({length:count},(_,i)=>i).filter(()=>rng()<.65);
    const config={position:[6378137+[50,300,1000,1e5][n%4],n%4===0?250:0,0],direction:[-1,n%3===0?.3:0,0],timeSinceMoved:n%2===0?.01:1,pixelRatio:n%3===0?2:1};
    for(const pass of [0,6,7]){const args=[json,options,ready,config,pass];traversal.cases.push({f:'traverse',a:enc(args),r:enc(await traverse(...args))});}
  }
  for(const scheme of ['QUADTREE','OCTREE'])for(const volume of [{box:[0,0,0,100,0,0,0,200,0,0,0,50]},{region:[2.8,-.4,-2.8,.6,-100,500]},{extensions:{'3DTILES_bounding_volume_S2':{token:'1',minimumHeight:0,maximumHeight:100}}},{extensions:{'3DTILES_bounding_volume_S2':{token:'3',minimumHeight:-100,maximumHeight:200}}}])for(const children of [0,1]){
    const json={asset:{version:'1.1'},geometricError:1000,root:{boundingVolume:volume,geometricError:100,refine:'REPLACE',contents:[{uri:'models/a/{level}/{x}/{y}/{z}.glb'},{uri:'models/b/{level}/{x}/{y}/{z}.glb'}],implicitTiling:{subdivisionScheme:scheme,subtreeLevels:2,availableLevels:6,subtrees:{uri:'subtrees/{level}/{x}/{y}/{z}.json'}}}};
    const subtree={tileAvailability:{constant:1},contentAvailability:[{constant:1},{constant:children}],childSubtreeAvailability:{constant:children}};
    const args=[json,subtree,{}];expansion.cases.push({f:'expand',a:enc(args),r:enc(await expand(...args))});
  }
  for(const scheme of ['QUADTREE','OCTREE']){
    const count=scheme==='QUADTREE'?5:9;
    const bytes=new Uint8Array(count*12),data=new DataView(bytes.buffer);
    for(let i=0;i<count;i++){data.setFloat32(i*4,i,true);data.setFloat32(count*4+i*4,100-i,true);data.setFloat32(count*8+i*4,20-i,true);}
    const json={asset:{version:'1.1'},geometricError:1000,schema:{classes:{tile:{properties:{min:{type:'SCALAR',componentType:'FLOAT32',semantic:'TILE_MINIMUM_HEIGHT'},max:{type:'SCALAR',componentType:'FLOAT32',semantic:'TILE_MAXIMUM_HEIGHT'},error:{type:'SCALAR',componentType:'FLOAT32',semantic:'TILE_GEOMETRIC_ERROR'}}}}},root:{boundingVolume:{region:[-.1,.2,.1,.3,-100,500]},geometricError:100,refine:'ADD',content:{uri:'models/{level}/{x}/{y}/{z}.glb'},implicitTiling:{subdivisionScheme:scheme,subtreeLevels:2,availableLevels:4,subtrees:{uri:'subtrees/{level}/{x}/{y}/{z}.json'}}}};
    const subtree={tileAvailability:{constant:1},contentAvailability:{constant:1},childSubtreeAvailability:{constant:0},buffers:[{uri:'meta.bin',byteLength:bytes.length}],bufferViews:[0,1,2].map(i=>({buffer:0,byteOffset:count*4*i,byteLength:count*4})),propertyTables:[{class:'tile',count,properties:{min:{values:0},max:{values:1},error:{values:2}}}],tileMetadata:0};
    const args=[json,subtree,{'meta.bin':Array.from(bytes)}];expansion.cases.push({f:'expand',a:enc(args),r:enc(await expand(...args))});
  }
  return [frame,view,traversal,expansion,derived,movement,dynamic];
}
