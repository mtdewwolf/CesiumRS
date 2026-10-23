// Actual CesiumJS methods with explicit host boundaries; no browser/GPU needed.
import { Atmosphere, Fog, Scene, Cesium3DTileset, Resource, Cartesian3, Matrix4,
  JulianDate, Color, Camera, GeographicProjection, Ellipsoid } from '@cesium/engine';
import {suite, enc, makeRng} from './lib.mjs';

function renderScenario(options, operations) {
  let trace=[], changed=true, failure;
  const log=x=>trace.push(x);
  const step=x=>{log(x);if(failure===x)throw new Error('injected');};
  const epoch=new JulianDate(2451545,0), time=t=>JulianDate.addSeconds(epoch,t,new JulianDate());
  const frame={frameNumber:options.frameNumber??0,time:time(0),afterRender:[],passes:{},camera:{canPreloadFlight:()=>false},creditDisplay:{
    update:()=>step('prePasses'),beginFrame:()=>log('creditBegin'),endFrame:()=>log('creditEnd')}};
  const primitives={show:true,prePassesUpdate:()=>step('primitivePre'),postPassesUpdate:()=>step('primitivePost'),
    updateForPass:(_,p)=>log(`pass:${p.pass}`),update:()=>{if(primitives.show)step('primitiveUpdate');}};
  const view={viewport:{},passState:{},checkForCameraUpdates:()=>{const c=changed;changed=false;return c;}};
  const scene={_frameState:frame,primitives,_view:view,_defaultView:view,_renderRequested:true,
    requestRenderMode:options.requestRenderMode??false,maximumRenderTimeChange:options.maximumRenderTimeChange??undefined,
    mode:3,rethrowRenderErrors:options.rethrow??false,_jobScheduler:{resetBudgets:()=>step('reset')},
    _picking:{update:()=>{},updateMostDetailedRayPicks:()=>step('mostDetailed')},
    _computeCommandList:[],_overlayCommandList:[],postProcessStages:{hasSelected:false},backgroundColor:Color.BLACK,
    fog:{update:()=>{}},context:{uniformState:{update:()=>{},updatePass:()=>{}},drawingBufferWidth:800,drawingBufferHeight:600,
      beginFrame:()=>step('begin'),endFrame:()=>step('end')},
    updateFrameState:()=>{},updateEnvironment:()=>step('environment'),
    updateAndExecuteCommands:()=>primitives.update(),resolveFramebuffers:()=>step('resolve'),
    requestRender(){this._renderRequested=true;}};
  for(const [field,name] of [['_preUpdate','preUpdate'],['_postUpdate','postUpdate'],['_preRender','preRender'],['_postRender','postRender'],['_renderError','renderError']])scene[field]={raiseEvent:()=>log(name)};
  const result=[];
  for(const [op,v] of operations) {
    switch(op) {
      case 'request':scene.requestRender();break;
      case 'camera':changed=true;break;
      case 'hdr':scene._hdrDirty=true;break;
      case 'log':scene._logDepthBufferDirty=true;break;
      case 'show':primitives.show=v;break;
      case 'fail':failure=v;break;
      case 'after':frame.afterRender.push(()=>{log('after');return v;});break;
      case 'render': {
        trace=[];let error=false;
        try{Scene.prototype.render.call(scene,time(v));}catch{error=true;}
        result.push({trace:[...trace],error,frame:frame.frameNumber,newFrame:frame.newFrame,requested:scene._renderRequested,
          time:JulianDate.secondsDifference(frame.time,epoch),last:scene._lastRenderTime?JulianDate.secondsDifference(scene._lastRenderTime,epoch):null});
        break;
      }
    }
  }
  return result;
}
function fogScenario(options, frames) {
  const fog=Object.assign(new Fog(),options), state={enabled:false,renderable:false,density:0,visualDensityScalar:0,sse:0,minimumBrightness:0};
  return frames.map(([mode,height,position,direction])=>{
    fog.update({mode,fog:state,camera:{positionCartographic:height===null?undefined:{height},positionWC:Cartesian3.fromArray(position),directionWC:Cartesian3.fromArray(direction)}});
    return {...state};
  });
}
async function tilesetSnapshot(json, options={}, url='https://example.com/tiles/tileset.json?key=test', matrix) {
  const original=Cesium3DTileset.loadJson;
  Cesium3DTileset.loadJson=async()=>structuredClone(json);
  try {
    const t=await Cesium3DTileset.fromUrl(new Resource(url),options);
    if(matrix)t.modelMatrix=Matrix4.fromArray(matrix);
    const sphere=t.boundingSphere, origin=t.clippingPlanesOriginMatrix;
    const tiles=[];const stack=[t.root];
    while(stack.length){const tile=stack.shift();tile.updateTransform(tile.parent?.computedTransform??t.modelMatrix);
      tiles.push({depth:tile._depth,refine:tile.refine,error:tile.geometricError,transform:Matrix4.toArray(tile.computedTransform),
        sphere:enc(tile.boundingSphere),uri:tile._contentResource?.url??null,empty:tile.hasEmptyContent});stack.push(...tile.children);}
    const result={url:t._url,basePath:t.basePath??null,asset:t.asset,extras:t.extras??null,properties:t.properties??null,
      sphere:enc(sphere),origin:Matrix4.toArray(origin),tiles,total:t.statistics.numberOfTilesTotal,
      allAdditive:t._allTilesAdditive,skip:t.isSkippingLevelOfDetail,up:t._modelUpAxis,forward:t._modelForwardAxis,
      credits:t._credits.map(c=>[c.html,c.showOnScreen]),geometricError:t._geometricError,
      maximumScreenSpaceError:t.maximumScreenSpaceError,memoryAdjusted:t.memoryAdjustedScreenSpaceError,
      progressive:t.progressiveResolutionHeightFraction};
    t.destroy();return result;
  } finally{Cesium3DTileset.loadJson=original;}
}
async function lifecycle(options, operations) {
  const original=Cesium3DTileset.loadJson;
  Cesium3DTileset.loadJson=async()=>({asset:{version:'1.1'},geometricError:100,root:{boundingVolume:{sphere:[0,0,0,10]},geometricError:0,content:{uri:'model.glb'}}});
  let t;
  try{t=await Cesium3DTileset.fromUrl('https://example.com/tileset.json',{dynamicScreenSpaceError:false,...options});}
  finally{Cesium3DTileset.loadJson=original;}
  const tile=t.root, trace=[], result=[];
  for(const name of ['tileLoad','tileUnload','allTilesLoaded','initialTilesLoaded'])t[name].addEventListener(()=>trace.push(name));
  t.loadProgress.addEventListener((a,b)=>trace.push(['progress',a,b]));
  t.tileFailed.addEventListener(()=>trace.push('failed'));
  const frame={frameNumber:0,time:new JulianDate(2451545,0),newFrame:true,afterRender:[],commandList:[]};
  let ready=false;
  for(const [op,value] of operations) {
    switch(op) {
      case 'install':tile._content={ready,featuresLength:2,pointsLength:3,trianglesLength:4,geometryByteLength:100,texturesByteLength:20,batchTableByteLength:5,
        update:()=>{},destroy:()=>{trace.push('destroy');},isDestroyed:()=>false};tile._contentState=2;t._processingQueue.push(tile);t.statistics.numberOfTilesProcessing++;break;
      case 'ready':ready=value;tile._content.ready=ready;break;
      case 'pre':frame.newFrame=value;frame.frameNumber++;frame.time=JulianDate.addSeconds(frame.time,1,new JulianDate());t.prePassesUpdate(frame);break;
      case 'post':t.postPassesUpdate(frame);break;
      case 'touch':t._cache.touch(tile);break;
      case 'trim':t.trimLoadedTiles();break;
      case 'after':{const callbacks=frame.afterRender.splice(0);trace.push(['callbacks',callbacks.map(fn=>fn())]);break;}
      case 'snapshot':result.push({trace:[...trace],state:tile._contentState,ready:t.statistics.numberOfTilesWithContentReady,processing:t.statistics.numberOfTilesProcessing,
        loaded:t.statistics.numberOfLoadedTilesTotal,memory:t.totalMemoryUsageInBytes,tilesLoaded:t.tilesLoaded,time:t.timeSinceLoad,adjusted:t.memoryAdjustedScreenSpaceError});trace.length=0;break;
    }
  }
  t.destroy();return result;
}
async function traversal(options, ready, pass=0) {
  const leaf=(x,id)=>({boundingVolume:{sphere:[6378137+x,0,0,5]},geometricError:0,content:{uri:`${id}.glb`}});
  const json={asset:{version:'1.1'},geometricError:1000,root:{...leaf(0,0),geometricError:100,refine:options.refine??'REPLACE',children:[leaf(-10,1),leaf(10,2)]}};
  const original=Cesium3DTileset.loadJson;Cesium3DTileset.loadJson=async()=>json;
  let t;try{t=await Cesium3DTileset.fromUrl('https://example.com/tileset.json',{dynamicScreenSpaceError:false,foveatedScreenSpaceError:false,cullWithChildrenBounds:false,cullRequestsWhileMoving:false,...options});}finally{Cesium3DTileset.loadJson=original;}
  const tiles=[t.root,...t.root.children];
  for(const [i,tile] of tiles.entries())if(ready.includes(i)){tile._contentState=3;tile._content={featurePropertiesDirty:false,destroy:()=>{},isDestroyed:()=>false};}
  const scene={drawingBufferWidth:800,drawingBufferHeight:600,canvas:{clientWidth:800,clientHeight:600},pixelRatio:1,mapMode2D:1,mapProjection:new GeographicProjection(),ellipsoid:Ellipsoid.WGS84,mode:3};
  const camera=new Camera(scene);camera.position=new Cartesian3(6379137,0,0);camera.direction=new Cartesian3(-1,0,0);camera.up=new Cartesian3(0,0,1);camera.right=new Cartesian3(0,1,0);camera.update(3);
  const frame={camera,context:{drawingBufferWidth:800,drawingBufferHeight:600},pixelRatio:1,mode:3,mapProjection:scene.mapProjection,
    cullingVolume:camera.frustum.computeCullingVolume(camera.positionWC,camera.directionWC,camera.upWC),frameNumber:1,time:new JulianDate(2451545,0),fog:{enabled:false},verticalExaggeration:1,verticalExaggerationRelativeHeight:0};
  t._pass=pass;t._updatedVisibilityFrame=1;t.getTraversal({pass}).selectTiles(t,frame);
  const result={selected:t._selectedTiles.map(tile=>tiles.indexOf(tile)),requested:t._requestedTiles.map(tile=>tiles.indexOf(tile)).sort(),visited:t.statistics.visited};
  t.destroy();return result;
}
export default async function sceneRuntime() {
  const scene=suite('Scene'),fog=suite('Fog'),atmosphere=suite('Atmosphere'),tileset=suite('Cesium3DTileset');
  for(const requestRenderMode of [false,true])for(const maximumRenderTimeChange of [null,0,.5,2]) {
    const o={requestRenderMode,maximumRenderTimeChange};
    scene.add('render',[o,[['render',0],['render',0],['render',.5],['render',1],['request'],['render',1],['render',-2],['after',true],['render',-2],['render',-2],['camera'],['render',-2],['hdr'],['render',-2],['log'],['render',-2],['show',false],['render',-2]]],renderScenario);
  }
  for(const fail of ['reset','primitivePre','prePasses','mostDetailed','begin','environment','primitiveUpdate','resolve','end','primitivePost'])for(const rethrow of [false,true])scene.add('render',[{requestRenderMode:true,rethrow},[['fail',fail],['render',0],['fail',null],['render',0]]],renderScenario);
  scene.add('render',[{frameNumber:15000000},[['render',0],['render',0]]],renderScenario);
  const rng=makeRng(0x5ce9e);
  for(let i=0;i<25;i++) {
    const ops=[];for(let j=0;j<12;j++){if(rng()<.3)ops.push(['request']);if(rng()<.2)ops.push(['camera']);ops.push(['render',Math.floor(rng()*10)/2]);}
    scene.add('render',[{requestRenderMode:true,maximumRenderTimeChange:.5},ops],renderScenario);
  }
  const frames=[...[0,1,10,1000,8e5,8e5+1,-100,null].flatMap(h=>[0,1,2,3].flatMap(mode=>[[mode,h,[6378137,0,0],[0,1,0]],[mode,h,[6378137,0,0],[-1,0,0]],[mode,h,[6378137,0,0],[-.6,.8,0]]]))];
  for(const o of [{},{enabled:false},{renderable:false},{heightFalloff:0},{heightFalloff:1.2,density:.01,visualDensityScalar:.8,screenSpaceErrorFactor:4,minimumBrightness:.1}])fog.add('update',[o,frames],fogScenario);
  for(const shifts of [[0,0,0],[1e-8,0,0],[1e-7,0,0],[1.01e-7,0,0],[0,-.1,0],[0,0,.2]])atmosphere.add('parameters',[shifts],hsb=>{
    const a=new Atmosphere();[a.hueShift,a.saturationShift,a.brightnessShift]=hsb;
    return {light:a.lightIntensity,rayleigh:enc(a.rayleighCoefficient),mie:enc(a.mieCoefficient),rayleighHeight:a.rayleighScaleHeight,mieHeight:a.mieScaleHeight,anisotropy:a.mieAnisotropy,dynamic:a.dynamicLighting,correct:Atmosphere.requiresColorCorrect(a)};});
  const leaf=(sphere=[0,0,0,10],error=0)=>({boundingVolume:{sphere},geometricError:error,content:{uri:'model.glb'}});
  const base={asset:{version:'1.1'},geometricError:100,root:{...leaf([0,0,0,20],20),refine:'REPLACE',children:[leaf([-10,0,0,5]),leaf([10,0,0,5])]}};
  const cases=[];
  for(const version of ['0.0','1.0','1.1','2.0'])cases.push([{...base,asset:{version}},{}]);
  for(const refine of ['ADD','REPLACE'])for(const skipLevelOfDetail of [false,true])cases.push([{...base,root:{...base.root,refine}},{skipLevelOfDetail}]);
  cases.push([{...base,asset:{version:'1.1',tilesetVersion:'v5',gltfUpAxis:'Z',extras:{cesium:{credits:[{html:'credit'}]}}},extras:{hello:1},properties:{height:{minimum:0,maximum:4}}},{showCreditsOnScreen:true}]);
  for(const boundingVolume of [{sphere:[6378137,0,0,0]},{box:[0,0,0,1,0,0,0,2,0,0,0,3]},{region:[.1,.2,.11,.21,0,100]}])cases.push([{...base,root:{boundingVolume,geometricError:0}},{}]);
  for(const progressiveResolutionHeightFraction of [-1,0,.3,.8])cases.push([base,{progressiveResolutionHeightFraction}]);
  cases.push([{...base,extensionsRequired:['NOT_SUPPORTED']},{}]);
  cases.push([{...base,asset:undefined},{}]);
  const matrix=Matrix4.toArray(Matrix4.fromUniformScale(2));matrix[12]=100;
  cases.push([base,{},'https://example.com/tiles/tileset.json?key=test',matrix]);
  for(const args of cases) {
    try{tileset.cases.push({f:'load',a:enc(args),r:enc(await tilesetSnapshot(...args))});}
    catch(e){tileset.cases.push({f:'load',a:enc(args),r:{throws:e.message,kind:e.name}});}
  }
  for(const cacheBytes of [0,100,125,1000]) {
    const args=[{cacheBytes,maximumCacheOverflowBytes:0},[['install'],['pre',true],['post'],['snapshot'],['after'],['snapshot'],['ready',true],['pre',true],['touch'],['post'],['snapshot'],['after'],['snapshot'],['pre',true],['post'],['snapshot'],['after'],['snapshot'],['trim'],['post'],['snapshot']]];
    tileset.cases.push({f:'lifecycle',a:enc(args),r:enc(await lifecycle(...args))});
  }
  for(const refine of ['ADD','REPLACE'])for(const ready of [[],[0],[1],[0,1],[1,2],[0,1,2]])for(const pass of [0,6,7]) {
    const args=[{refine},ready,pass];
    try{tileset.cases.push({f:'traversal',a:enc(args),r:enc(await traversal(...args))});}
    catch(e){throw new Error(`Traversal fixture failed: ${e.stack}`);}
  }
  return [scene,fog,atmosphere,tileset];
}
