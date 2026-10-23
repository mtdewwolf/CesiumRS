import { BoundingRectangle, Cartesian2, Cartesian3, Cartesian4, Ellipsoid, GeographicProjection, WebMercatorProjection, Matrix4, PerspectiveFrustum, OrthographicFrustum, OrthographicOffCenterFrustum } from '@cesium/engine';
import Camera from '@cesium/engine/Source/Scene/Camera.js';
import SceneTransforms from '@cesium/engine/Source/Scene/SceneTransforms.js';
import { suite } from './lib.mjs';

const arr3=p=>[p.x,p.y,p.z];
const arr2=p=>[p.x,p.y];
export function scene(o) {
  const projection = o.mercator ? new WebMercatorProjection() : new GeographicProjection();
  const s = {canvas:{clientWidth:800,clientHeight:600},drawingBufferWidth:1600,drawingBufferHeight:900,mapProjection:projection,ellipsoid:Ellipsoid.WGS84,
    mode:o.mode,pixelRatio:1,mapMode2D:1,screenSpaceCameraController:{minimumZoomDistance:1,maximumZoomDistance:Infinity},frameState:{mode:o.mode,morphTime:o.morph??1,mapProjection:projection}};
  const c = s.camera = new Camera(s);
  if(o.mode===2)c.frustum=new OrthographicOffCenterFrustum({left:-1e7,right:1e7,top:7.5e6,bottom:-7.5e6,near:1,far:1e9});
  else if(o.ortho)c.frustum=new OrthographicFrustum({width:2e7,aspectRatio:4/3,near:1,far:1e9});
  else c.frustum=new PerspectiveFrustum({fov:Math.PI/3,aspectRatio:4/3,near:1,far:1e9});
  c.position=new Cartesian3(...o.position);c.direction=new Cartesian3(0,0,-1);c.up=new Cartesian3(0,1,0);c.right=new Cartesian3(1,0,0);c.update(o.mode);
  return s;
}
function project(o, positions, eye) {
  const s=scene(o),c=s.camera;
  const before=[...arr3(c.position),...Matrix4.toArray(c.viewMatrix),...Matrix4.toArray(c.frustum.projectionMatrix)];
  const outputs=positions.map(p=>{
    const point=new Cartesian3(...p),offset=new Cartesian3(...eye);
    const actual=SceneTransforms.computeActualEllipsoidPosition(s.frameState,point);
    const window=SceneTransforms.worldWithEyeOffsetToWindowCoordinates(s,point,offset);
    return {actual:actual?arr3(actual):null,window:window?arr2(window):null,buffer:window?arr2(SceneTransforms.transformWindowToDrawingBuffer(s,window)):null};
  });
  const after=[...arr3(c.position),...Matrix4.toArray(c.viewMatrix),...Matrix4.toArray(c.frustum.projectionMatrix)];
  return {outputs,unchanged:before.every((v,i)=>v===after[i])};
}
function unproject(o, viewport, range, position, depth, log) {
  const s=scene(o),c=s.camera;
  s.view={passState:{viewport:new BoundingRectangle(...viewport)}};
  s.frameState.useLogDepth=log;
  const frustum=c.frustum.clone();frustum.near=range[0];frustum.far=range[1];
  s.context={uniformState:{currentFrustum:new Cartesian2(...range),log2FarDepthFromNearPlusOne:Math.log2(range[1]-range[0]+1),inverseView:c.inverseViewMatrix,
    inverseViewProjection: o.mode===2||o.ortho ? Matrix4.IDENTITY : Matrix4.inverse(Matrix4.multiply(frustum.projectionMatrix,c.viewMatrix,new Matrix4()),new Matrix4())}};
  return arr3(SceneTransforms.drawingBufferToWorldCoordinates(s,new Cartesian2(...position),depth));
}
export default function sceneTransforms() {
  const t=suite('SceneTransforms');
  const positions=[[0,0,0],[6378137,0,0],[-6378137,0,100],[1,1,1],[4510023,4510023,1000000],[0,0,7356752]];
  for(const mercator of [false,true])for(const mode of [0,1,2,3])for(const ortho of [false,true])for(const morph of mode===0?[0,.3,1]:[1])for(const x of mode===2?[-1.8e7,0,1.8e7]:[0])for(const eye of [[0,0,0],[100,-200,300]]) {
    const o={mode,mercator,ortho,morph,position:[x,0,2e7]};t.add('project',[o,positions,eye],project);
  }
  for(const mode of [1,2,3])for(const ortho of [false,true])for(const range of [[1,1e9],[100,10000]])for(const viewport of [[0,0,1600,900],[100,200,640,480]])for(const depth of [0,.25,.9,1])for(const log of [false,true]) {
    t.add('unproject',[{mode,ortho,position:[0,0,2e7]},viewport,range,[400,300],depth,log],unproject);
  }
  for(const p of [[2,4,6,2],[-4,7,-2,-1],[0,0,0,1]])t.add('clip',[[10,20,800,600],p],(v,p)=>arr2(SceneTransforms.clipToGLWindowCoordinates(new BoundingRectangle(...v),new Cartesian4(...p))));
  return [t];
}
