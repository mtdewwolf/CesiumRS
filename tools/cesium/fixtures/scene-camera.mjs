// Real Camera methods, using a small Scene host with explicit canvas and
// projection inputs. No camera calculations are implemented in this adapter.
import Camera from '@cesium/engine/Source/Scene/Camera.js';
import CameraFlightPath from '@cesium/engine/Source/Scene/CameraFlightPath.js';
import TweenCollection from '@cesium/engine/Source/Scene/TweenCollection.js';
import { Cartesian2, Cartesian3, Cartesian4, Cartographic, Matrix4, Rectangle,
  HeadingPitchRange, HeadingPitchRoll, BoundingSphere, Ellipsoid,
  GeographicProjection, WebMercatorProjection, PerspectiveFrustum,
  OrthographicFrustum, OrthographicOffCenterFrustum, Transforms, EasingFunction } from '@cesium/engine';
import { suite, makeRng } from './lib.mjs';

const c3 = a => Cartesian3.fromArray(a);
const c4 = a => Cartesian4.fromArray(a);
const arr = p => p === undefined ? null : [p.x, p.y, p.z];
const matrix = m => Matrix4.toArray(m);
const sphere = s => new BoundingSphere(c3(s[0]), s[1]);
const rectangle = r => new Rectangle(...r);
const offset = o => o.hpr ? new HeadingPitchRange(...o.hpr) : c3(o.cartesian);
function frustum(o) {
  let f;
  if (o.type === 'perspective') { f = new PerspectiveFrustum(); f.fov = o.fov ?? Math.PI/3; }
  else if (o.type === 'orthographic') { f = new OrthographicFrustum(); f.width = o.width; }
  else { f = new OrthographicOffCenterFrustum(); for (const k of ['left','right','top','bottom']) f[k] = o[k]; }
  if ('aspectRatio' in o) f.aspectRatio = o.aspectRatio;
  for (const k of ['near','far','xOffset','yOffset']) if (k in o) f[k] = o[k];
  return f;
}
function frustumValue(f) {
  if (f instanceof PerspectiveFrustum) return {type:'perspective', fov:f.fov, aspectRatio:f.aspectRatio, near:f.near, far:f.far, xOffset:f.xOffset, yOffset:f.yOffset};
  if (f instanceof OrthographicFrustum) return {type:'orthographic', width:f.width, aspectRatio:f.aspectRatio, near:f.near, far:f.far};
  return {type:'offCenter', left:f.left, right:f.right, top:f.top, bottom:f.bottom, near:f.near, far:f.far};
}
function snapshot(c) {
  const position = arr(c.positionWC), direction = arr(c.directionWC), up = arr(c.upWC), right = arr(c.rightWC);
  const p = c.positionCartographic;
  const cartographic = p ? {longitude:p.longitude, latitude:p.latitude, height:p.height} : null;
  const view = matrix(c.viewMatrix), inverseView = matrix(c.inverseViewMatrix);
  const hpr = {heading:c.heading ?? null, pitch:c.pitch ?? null, roll:c.roll ?? null};
  return {position:arr(c.position),direction:arr(c.direction),up:arr(c.up),right:arr(c.right),
    positionWC:position,directionWC:direction,upWC:up,rightWC:right,cartographic,view,inverseView,
    transform:matrix(c.transform),inverseTransform:matrix(c.inverseTransform),frustum:frustumValue(c.frustum),hpr,magnitude:c.getMagnitude() ?? null};
}
export function runCameraScenario(options, operations) {
  const ellipsoid = options.ellipsoid ? new Ellipsoid(...options.ellipsoid) : Ellipsoid.WGS84;
  const scene = {
    drawingBufferWidth:options.width ?? 800, drawingBufferHeight:options.height ?? 600,
    canvas:{clientWidth:options.clientWidth ?? options.width ?? 800, clientHeight:options.clientHeight ?? options.height ?? 600},
    pixelRatio:options.pixelRatio ?? 1, mapMode2D:options.mapMode2D ?? 1,
    mapProjection: options.projection === 'mercator' ? new WebMercatorProjection(ellipsoid) : new GeographicProjection(ellipsoid),
    ellipsoid:options.sceneEllipsoid ? new Ellipsoid(...options.sceneEllipsoid) : ellipsoid,mode:options.mode ?? 3,
    screenSpaceCameraController:{minimumZoomDistance:options.minZoom ?? 1, maximumZoomDistance:options.maxZoom ?? Infinity},
    pickPositionSupported:!!options.depth,
    pickPositionWorldCoordinates:() => options.depth ? c3(options.depth) : undefined,
    globe:options.globe ? {pickWorldCoordinates:() => c3(options.globe)} : undefined,
  };
  let camera = new Camera(scene);
  scene.camera = camera;
  scene.tweens = new TweenCollection();
  scene.screenSpaceCameraController.enableInputs = true;
  const callbacks = [];
  scene.completeMorph = () => callbacks.push({type:'morph'});
  let pathTween, correctionTween;
  if (options.frustum) camera.frustum = frustum(options.frustum);
  if (options.pose) {
    [camera.position,camera.direction,camera.up,camera.right] = options.pose.map(c3);
  }
  camera.update(scene.mode);
  if (options.transform) camera._setTransform(Matrix4.fromArray(options.transform));
  if (options.constrainedAxis) camera.constrainedAxis = c3(options.constrainedAxis);
  camera.percentageChanged = options.percentageChanged ?? 0.5;
  const events = [];
  if (options.events) camera.changed.addEventListener(x => events.push(x));
  const results = [];
  const flightOptions = a => {
    const o = {...a};
    if(o.destination) o.destination=Array.isArray(o.destination)?c3(o.destination):rectangle(o.destination.rectangle);
    if(o.orientation?.direction) o.orientation={direction:c3(o.orientation.direction),up:c3(o.orientation.up)};
    if(o.endTransform) o.endTransform=Matrix4.fromArray(o.endTransform);
    if(o.offset) o.offset=new HeadingPitchRange(...o.offset);
    if(o.easingFunction) o.easingFunction=o.easingFunction==='CUSTOM_SQUARE'?t=>t*t:EasingFunction[o.easingFunction];
    if(!o.noCallbacks) for(const type of ['complete','cancel']) o[type]=() => {
      callbacks.push({type,label:a.label??'',active:camera._currentFlight!==undefined,inputs:scene.screenSpaceCameraController.enableInputs});
      if(type==='complete'&&a.completeDestination) camera.flyTo(flightOptions({destination:a.completeDestination,duration:1,label:'chained'}));
    };
    return o;
  };
  const descriptor = t => t ? {duration:t.duration,easing:t.easingFunction ? Object.entries(EasingFunction).find(([,f])=>f===t.easingFunction)?.[0]??'CUSTOM_SQUARE':null,start:t.startObject,stop:t.stopObject} : null;
  for (const [name, ...args] of operations) {
    try {
      let result = null;
      if (name === 'flyTo') camera.flyTo(flightOptions(args[0]));
      else if(name==='flyToBoundingSphere') camera.flyToBoundingSphere(sphere(args[0]),flightOptions(args[1]??{}));
      else if(name==='advance') scene.tweens.update(args[0]);
      else if(name==='flightState') result={active:camera._currentFlight!==undefined,preload:camera.canPreloadFlight(),inputs:scene.screenSpaceCameraController.enableInputs,count:scene.tweens.length,
        planes:scene.preloadFlightCullingVolume?.planes.map(p=>[p.x,p.y,p.z,p.w])??null,callbacks:[...callbacks]};
      else if(name==='preloadCamera') scene.preloadFlightCamera=Camera.clone(camera);
      else if(name==='path') { pathTween=CameraFlightPath.createTween(scene,flightOptions(args[0])); result=descriptor(pathTween); }
      else if(name==='pathUpdate') pathTween.update({time:args[0]});
      else if(name==='pathComplete') pathTween.complete?.();
      else if(name==='pathCancel') pathTween.cancel?.();
      else if(name==='correction') { correctionTween=camera.createCorrectPositionTween(args[0]); result=descriptor(correctionTween); }
      else if(name==='correctionUpdate') correctionTween.update({time:args[0]});
      else if (name === 'setView') {
        const o = {...args[0]};
        if (o.destination) o.destination = Array.isArray(o.destination) ? c3(o.destination) : rectangle(o.destination.rectangle);
        if (o.orientation?.direction) o.orientation = {direction:c3(o.orientation.direction),up:c3(o.orientation.up)};
        if (o.endTransform) o.endTransform = Matrix4.fromArray(o.endTransform);
        camera.setView(o);
      } else if (name === 'setTransform') camera._setTransform(Matrix4.fromArray(args[0]));
      else if (name === 'lookAt') camera.lookAt(c3(args[0]), offset(args[1]));
      else if (name === 'lookAtTransform') camera.lookAtTransform(Matrix4.fromArray(args[0]), args[1] ? offset(args[1]) : undefined);
      else if (name === 'rectangle') result = arr(camera.getRectangleCameraCoordinates(rectangle(args[0])));
      else if (name === 'pick') result = arr(camera.pickEllipsoid(new Cartesian2(...args[0]), ellipsoid));
      else if (name === 'ray') { const r = camera.getPickRay(new Cartesian2(...args[0])); result = r ? {origin:arr(r.origin),direction:arr(r.direction)} : null; }
      else if (name === 'sphere') result = [camera.distanceToBoundingSphere(sphere(args[0])),camera.getPixelSize(sphere(args[0]),scene.drawingBufferWidth,scene.drawingBufferHeight)];
      else if (name === 'viewBoundingSphere') camera.viewBoundingSphere(sphere(args[0]), args[1] ? new HeadingPitchRange(...args[1]) : undefined);
      else if (name === 'computeViewRectangle') { const r = camera.computeViewRectangle(ellipsoid); result = r ? [r.west,r.south,r.east,r.north] : null; }
      else if (name === 'move' || name === 'look' || name === 'rotate') camera[name](c3(args[0]), args[1] ?? undefined);
      else if (name === 'worldToCameraCoordinates' || name === 'cameraToWorldCoordinates') {
        const p = camera[name](c4(args[0])); result = [p.x,p.y,p.z,p.w];
      } else if (/^(worldToCamera|cameraToWorld)Coordinates(Point|Vector)$/.test(name)) result = arr(camera[name](c3(args[0])));
      else if (name === 'state') result = snapshot(camera);
      else if (name === 'pose') [camera.position,camera.direction,camera.up,camera.right] = args[0].map(c3);
      else if (name === 'update') { scene.mode = args[0]; camera.update(args[0]); }
      else if (name === 'frustum') camera.frustum = frustum(args[0]);
      else if (name === 'clone') { camera = Camera.clone(camera); scene.camera=camera; }
      else if (name === 'changed') {
        const original = globalThis.performance;
        try { globalThis.performance = {now:() => args[0]}; camera._updateCameraChanged(); }
        finally { globalThis.performance = original; }
        result = {delta:camera.positionWCDeltaMagnitude, last:camera.positionWCDeltaMagnitudeLastFrame,time:camera.timeSinceMoved,events:[...events]};
      } else camera[name](args[0] ?? undefined);
      results.push({ok:result});
    } catch(e) { results.push({throws:e.name}); }
  }
  return {results, camera:snapshot(camera)};
}

export default function sceneCamera() {
  const s = suite('Camera');
  const add = (o, ops) => s.add('scenario', [o,ops],runCameraScenario);
  const point = (lon,lat,h=0) => arr(Cartesian3.fromDegrees(lon,lat,h));
  // Constructor, CSS/drawing-buffer sizes, projections and central bodies.
  for (const o of [{},{width:600,height:900},{width:1920,height:1080,pixelRatio:2,clientWidth:960,clientHeight:540},
    {projection:'mercator'},{ellipsoid:[1737400,1737400,1737400]}]) add(o,[['state']]);
  const moves = ['moveForward','moveBackward','moveUp','moveDown','moveRight','moveLeft','zoomIn','zoomOut'];
  const turns = ['lookLeft','lookRight','lookUp','lookDown','twistLeft','twistRight','rotateLeft','rotateRight','rotateUp','rotateDown'];
  for (const mode of [3,1,2]) for (const mapMode2D of [0,1]) {
    const o = {mode,mapMode2D,pose:[[1000000,2000000,3000000],[0,0,-1],[0,1,0],[1,0,0]]};
    if (mode === 2) o.frustum = {type:'offCenter',left:-1000000,right:1000000,top:750000,bottom:-750000};
    for (const name of moves) for (const amount of [null,0,12345,-5000,1e9]) add(o,[[name,amount]]);
    for (const name of turns) for (const amount of [null,0,0.2,-0.7]) add(o,[[name,amount]]);
    add(o,[...moves.map(name=>[name,123]),...turns.map(name=>[name,0.1])]);
    for (const window of [[400,300],[0,0],[800,600],[-200,350],[1500,-400]]) add(o,[['ray',window],['pick',window]]);
    for (const r of [[-1,-.5,1,.8],[3,-.3,-3,.4],[-Math.PI,-Math.PI/2,Math.PI,Math.PI/2]]) add(o,[['rectangle',r],['setView',{destination:{rectangle:r}}],['computeViewRectangle']]);
  }
  // Cartesian and HPR setView; rectangle fitting across equator and dateline.
  for (const mode of [3,1,2]) for (const projection of ['geographic','mercator']) {
    const o = {mode,projection};
    if (mode === 2) o.frustum = {type:'offCenter',left:-1000000,right:1000000,top:750000,bottom:-750000};
    for (const destination of [point(0,0,1000),point(135,-30,500000),point(-80,75,2e6)]) {
      for (const orientation of [{},{heading:.7,pitch:-.6,roll:.3}]) add(o,[['setView',{destination,orientation}],['ray',[400,300]],['pick',[400,300]],['computeViewRectangle']]);
    }
    for (const r of [[-.8,.2,-.4,.5],[3.05,-.3,-3.05,.3],[-2,-1,2,1],[-Math.PI,-Math.PI/2,Math.PI,Math.PI/2]]) add(o,[['rectangle',r]]);
  }
  const targets = [point(0,0),point(123,45),point(-75,-60)];
  for (const target of targets) for (const off of [{cartesian:[1000,2000,3000]},{cartesian:[0,0,5000]},{hpr:[.5,-.7,1e6]},{hpr:[8,-2,3000]}]) {
    add({},[['lookAt',target,off],['state'],['setTransform',matrix(Matrix4.IDENTITY)],['ray',[400,300]],['pick',[400,300]],['computeViewRectangle']]);
    add({},[['viewBoundingSphere',[target,10000],off.hpr],['sphere',[target,10000]]]);
  }
  // All six coordinate conversions, a translated rigid frame and w != 1.
  const transforms = [matrix(Matrix4.IDENTITY), matrix(Matrix4.fromTranslation(c3(point(30,40))))];
  transforms.push(matrix(Transforms.eastNorthUpToFixedFrame(c3(point(-75,40,100)))));
  for (const transform of transforms) for (const mode of [3,1,2]) {
    const o = {mode}; if(mode===2) o.frustum={type:'offCenter',left:-4e6,right:4e6,top:3e6,bottom:-3e6};
    add(o,[['setTransform',transform], ...['worldToCameraCoordinates','cameraToWorldCoordinates'].map(name=>[name,[1,2,3,.3]]),
      ...['worldToCameraCoordinatesPoint','cameraToWorldCoordinatesPoint','worldToCameraCoordinatesVector','cameraToWorldCoordinatesVector'].map(name=>[name,[1000,-2000,3000]]),['clone']]);
  }
  for(const axis of [[0,0,1],[0,1,0],[1,0,0]]) for(const angle of [-4,-.3,.3,4]) add({constrainedAxis:axis},[['rotateUp',angle],['rotateLeft',angle]]);
  for(const axis of [[0,0,1],[0,0,-1]]) for(const angle of [-.3,.3]) {
    add({constrainedAxis:[0,0,1],pose:[axis.map(x=>x*1e7),axis.map(x=>-x),[0,1,0],axis[2]>0?[1,0,0]:[-1,0,0]]},[['rotateUp',angle]]);
  }
  for(const transform of transforms) for(const mode of [3,1,2]) {
    const o={mode};if(mode===2)o.frustum={type:'offCenter',left:-2e6,right:2e6,top:1e6,bottom:-1e6};
    add(o,[['setView',{destination:point(35,-25,50000),endTransform:transform,orientation:{direction:[0,0,-1],up:[0,1,0]}}],['state'],
      ['look',[1,2,3],.17],['move',[.3,-.2,1],500],['rotate',[1,0,1],.15],['ray',[100,400]]]);
  }
  for(const projection of ['geographic','mercator']) for(const mode of [1,2]) {
    const o={mode,projection};if(mode===2)o.frustum={type:'offCenter',left:-2e6,right:2e6,top:1e6,bottom:-1e6};
    add(o,[['setView',{destination:[100000,200000,300000],convert:false,orientation:{heading:.6,pitch:-.5,roll:.2}}],['state'],['pick',[400,300]]]);
  }
  for(const transform of transforms.slice(1)) for(const mode of [1,2]) {
    const o={mode};if(mode===2)o.frustum={type:'offCenter',left:-2e6,right:2e6,top:1e6,bottom:-1e6};
    for(const off of [{cartesian:[1e6,2e6,3e6]},{hpr:[.6,-.4,3e6]}]) add(o,[['lookAtTransform',transform,off],['ray',[400,300]],['pick',[400,300]]]);
  }
  for(const projection of ['geographic','mercator']) for(const aspectRatio of [.5,2]) {
    const o={projection,frustum:{type:'orthographic',width:1e6,aspectRatio}};
    add(o,[['setView',{destination:point(80,40,1e6)}],['state'],['ray',[100,200]],['pick',[100,200]],['computeViewRectangle'],['rectangle',[-.7,.1,.8,.9]]]);
    add(o,[['viewBoundingSphere',[targets[1],1e5]],['sphere',[targets[1],1e5]],['lookAt',targets[0],{cartesian:[1e6,0,1e6]}]]);
  }
  add({pose:[[7e6,0,0],[-2,0,0],[.1,0,3],[0,2,0]]},[['state'],['ray',[400,300]]]);
  add({},[['setView',{destination:point(0,0,1e6),orientation:{direction:[1,0,0],up:[0,0,1]}}],['computeViewRectangle']]);
  add({},[['changed',1000],['changed',2000],['moveUp',20],['changed',3000],['changed',4000]]);
  for(const o of [{},{depth:point(-80,20,1000)},{globe:point(-80,20,2000),depth:point(-80,20,1000)}]) add(o,[['switchToOrthographicFrustum'],['moveForward',1000],['rotateUp',.1],['switchToPerspectiveFrustum']]);
  for(const o of [{clientWidth:0},{clientHeight:0}]) add(o,[['pick',[0,0]],['ray',[0,0]]]);
  for(const radius of [0,1,1000,1e6]) add({minZoom:50,maxZoom:200000},[['viewBoundingSphere',[targets[1],radius]],['sphere',[targets[1],radius]]]);
  add({},[['update',2],['update',0],['lookAt',targets[0],{cartesian:[1,2,3]}],['pick',[400,300]],['rectangle',[-1,-.5,1,.5]],['setView',{destination:targets[1]}]]);
  for(const mode of [3,1,2]) {
    const o = {mode,events:true,percentageChanged:.05}; if(mode===2) o.frustum={type:'offCenter',left:-4e6,right:4e6,top:3e6,bottom:-3e6};
    add(o,[['changed',1000],['moveRight',500000],['changed',2000],['changed',3000],['lookRight',.5],['changed',4000],['zoomOut',5000000],['changed',5000]]);
  }
  // Seeded multi-step sequences exercise lazy cache invalidation.
  const rng = makeRng(0xcafe1234);
  for(let i=0;i<100;i++) {
    const operations=[['setView',{destination:point(rng()*360-180,rng()*160-80,10000+rng()*2e6),orientation:{heading:rng()*6,pitch:-.1-rng()*1.4,roll:rng()*.5}}]];
    for(let j=0;j<8;j++) {
      const name = [...moves,...turns][Math.floor(rng()*(moves.length+turns.length))];
      operations.push([name,moves.includes(name)?(rng()-.5)*10000:(rng()-.5)*.1]);
      if(j%3===0) operations.push(['state']);
    }
    operations.push(['ray',[rng()*800,rng()*600]],['computeViewRectangle']);
    add({},operations);
  }
  return [s];
}
