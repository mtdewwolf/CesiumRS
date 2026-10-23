import {Cartesian3,Matrix4,Transforms} from '@cesium/engine';
import {suite,makeRng} from './lib.mjs';
import {runCameraScenario} from './scene-camera.mjs';

export default function sceneCameraFlights() {
  const camera=suite('CameraFlights'),path=suite('CameraFlightPath');
  const add=(o,ops)=>camera.add('scenario',[o,ops],runCameraScenario);
  const direct=(o,ops)=>path.add('scenario',[o,ops],runCameraScenario);
  const point=(lon,lat,h=0)=>{const p=Cartesian3.fromDegrees(lon,lat,h);return [p.x,p.y,p.z];};
  const flat=mode=>mode===2?{frustum:{type:'offCenter',left:-4e6,right:4e6,top:3e6,bottom:-3e6}}:{};
  const frames=[0,.1,.25,.5,.75,.9,1,1.1];
  const sample=times=>times.flatMap(t=>[['advance',t],['state'],['flightState']]);
  for(const mode of [3,1,2])for(const projection of ['geographic','mercator'])for(const mapMode2D of [0,1]) {
    const o={mode,projection,mapMode2D,...flat(mode)};
    for(const duration of [undefined,0,-1,1,3]) {
      const flight={destination:point(100,-30,5e5),orientation:{heading:1,pitch:-.5,roll:.3},duration};
      add(o,[['flyTo',flight],['flightState'],...sample([5,5.25,5.5,6,7,8,9])]);
    }
    add(o,[['flyTo',{destination:{rectangle:[2.8,-.3,-2.9,.8]},duration:2}],['flightState'],...sample([0,.5,1,2])]);
    for(const easingFunction of ['LINEAR_NONE','QUINTIC_IN_OUT','CUBIC_OUT','CUSTOM_SQUARE']) add(o,[['flyTo',{destination:point(-40,60,1e6),duration:1,easingFunction}],...sample(frames)]);
    for(const cancel of ['cancelFlight','completeFlight'])add(o,[['flyTo',{destination:point(85,30,2e6),duration:2,label:'a'}],['advance',10],['advance',10.7],['flightState'],[cancel],['state'],['flightState'],['advance',20],['flightState'],[cancel]]);
    add(o,[['flyTo',{destination:point(85,30,2e6),duration:2,label:'a'}],['advance',0],['advance',.5],['flyTo',{destination:point(-30,-50,4e5),duration:1,label:'b'}],['flightState'],...sample([.5,1,1.5])]);
    add(o,[['flyHome',1],['flightState'],...sample([0,.5,1])]);
    for(const radius of [0,1e5])for(const offset of [undefined,[.5,-1.5707963267948966,0],[.7,-.6,3e6]]) add(o,[['flyToBoundingSphere',[point(40,-20),radius],{duration:1,offset}],['flightState'],...sample([0,.5,1])]);
  }
  // Height arches, pitch adjustment, short/long routes and heading wrap.
  for(const mode of [3,1,2])for(const opts of [
    {},{maximumHeight:1e7},{maximumHeight:100},{maximumHeight:2e7,pitchAdjustHeight:1e5},
    {flyOverLongitude:0},{flyOverLongitude:Math.PI},{flyOverLongitude:Math.PI,flyOverLongitudeWeight:.5},
    {flyOverLongitude:Math.PI,flyOverLongitudeWeight:10},{flyOverLongitude:1e5,maximumHeight:2e7,pitchAdjustHeight:1},
  ]) {
    const o={mode,...flat(mode)};
    const options={destination:point(-170,-40,1000),orientation:{heading:.1,pitch:-.3,roll:.1},duration:2,...opts};
    direct(o,[['setView',{destination:point(170,25,1e6),orientation:{heading:6.2,pitch:-.8,roll:6.1}}],['path',options],
      ...[0,.2,.5,1,1.5,1.8,2].flatMap(t=>[['pathUpdate',t],['state']]),['pathComplete'],['flightState']]);
  }
  // Direct descriptor zero-duration behavior is deferred until complete.
  for(const mode of [3,1,2,0])for(const duration of [0,-1])direct({mode,...flat(mode)},[['path',{destination:point(20,40,1000),duration}],['flightState'],['pathComplete'],['state'],['flightState'],['pathCancel'],['flightState']]);
  for(const mode of [3,1,2]){
    const o={mode,...flat(mode)};
    const destination=mode===3?point(20,30,1000):[1000,2000,3000];
    direct(o,[['setView',{destination,convert:mode===3}],['path',{destination,convert:mode===3}],['flightState'],['pathComplete'],['flightState']]);
    add(o,[['setView',{destination,convert:mode===3}],['flyTo',{destination,convert:mode===3}],['flightState']]);
  }
  // Transformed flights and pre-existing preload camera reuse.
  const endTransform=Matrix4.toArray(Transforms.eastNorthUpToFixedFrame(Cartesian3.fromDegrees(25,45)));
  for(const mode of [3,1,2])add({mode,...flat(mode)},[['preloadCamera'],['flyTo',{destination:point(30,40,1e5),duration:1,endTransform,orientation:{direction:[-1,0,0],up:[0,0,1]}}],['flightState'],...sample([0,.5,1])]);
  add({},[['flyTo',{destination:point(10,20,3e5),duration:1,label:'first',completeDestination:point(50,10,2e5)}],...sample([0,1,1.5,2])]);
  for(const o of [{sceneEllipsoid:[1737400,1737400,1737400]},{ellipsoid:[1737400,1737400,1737400]}]) {
    add(o,[['lookAt',point(35,55),{hpr:[.3,-.4,1e5]}],['state'],['flyToBoundingSphere',[point(10,65),1e5],{duration:1}],...sample([0,.5,1])]);
  }
  for(const mode of [3,1,2])for(const duration of [0,1]) {
    const o={mode,...flat(mode)};
    add(o,[['flyTo',{destination:point(40,20,1e5),duration,noCallbacks:true}],...sample([0,.5,1])]);
  }
  for(const mode of [3,1])for(const aspectRatio of [.75,2]) {
    const o={mode,frustum:{type:'orthographic',width:8e6,aspectRatio}};
    add(o,[['flyTo',{destination:point(-40,50,1e6),duration:2,maximumHeight:1e7}],...sample([0,.5,1,2]),['correction',1]]);
    add(o,[['flyToBoundingSphere',[point(25,45),1e5],{duration:1}],...sample([0,.5,1])]);
  }
  for(const mode of [3,1,2]) {
    const o={mode,...flat(mode)};
    const destination=mode===3?point(20,30,1000):[1000,2000,3000];
    direct(o,[['setView',{destination,convert:mode===3}],['path',{destination,heading:1,pitch:-.3,roll:.6,convert:mode===3}],
      ['pathUpdate',-.2],['state'],['pathUpdate',1],['state'],['pathUpdate',3.1],['state'],['pathCancel'],['flightState']]);
    add({...o,minZoom:1e6,maxZoom:2e6},[['flyToBoundingSphere',[point(20,30),0],{duration:0}],['flightState']]);
  }
  add({},[['flyTo',{destination:point(100,-30,3e5),duration:1}],...sample([10,9,10.5,12])]);
  add({mode:0},[['flyTo',{destination:point(100,-30,3e5),duration:0}],['flightState'],['flyHome',0],['flightState'],['cancelFlight'],['completeFlight']]);
  for(const mode of [3,1,2])for(const pose of [
    [[8e7,7e7,3e6],[0,0,-1],[0,1,0],[1,0,0]],
    [[-8e7,-7e7,3e6],[0,0,-1],[0,1,0],[1,0,0]],
    [[1000,2000,3e6],[0,0,-1],[0,1,0],[1,0,0]],
  ]) {
    const o={mode,pose,...flat(mode)};
    const exists=mode===1&&Math.abs(pose[0][0])>1e7;
    add(o,[['correction',2],...(exists?[0,.25,.5,1].flatMap(t=>[['correctionUpdate',t],['state']]):[])]);
  }
  const rng=makeRng(0xf117);
  for(let i=0;i<50;i++) {
    const mode=[3,1,2][i%3],o={mode,...flat(mode)};
    add(o,[['flyTo',{destination:point(rng()*360-180,rng()*150-75,1e4+rng()*2e6),duration:.5+rng()*3,
      orientation:{heading:rng()*6.28,pitch:-rng()*1.5,roll:rng()*6.28},maximumHeight:rng()*2e7,pitchAdjustHeight:1e5}],...sample([0,.25,.5,1,2,3,5])]);
  }
  return [camera,path];
}
