import Transforms from '@cesium/engine/Source/Core/Transforms.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import Cartesian2 from '@cesium/engine/Source/Core/Cartesian2.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Cartesian4 from '@cesium/engine/Source/Core/Cartesian4.js';
import Matrix2 from '@cesium/engine/Source/Core/Matrix2.js';
import Matrix3 from '@cesium/engine/Source/Core/Matrix3.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import BoundingRectangle from '@cesium/engine/Source/Core/BoundingRectangle.js';
import Ellipsoid from '@cesium/engine/Source/Core/Ellipsoid.js';
import GeographicProjection from '@cesium/engine/Source/Core/GeographicProjection.js';
import WebMercatorProjection from '@cesium/engine/Source/Core/WebMercatorProjection.js';
import Cartographic from '@cesium/engine/Source/Core/Cartographic.js';
import PerspectiveFrustum from '@cesium/engine/Source/Core/PerspectiveFrustum.js';
import PerspectiveOffCenterFrustum from '@cesium/engine/Source/Core/PerspectiveOffCenterFrustum.js';
import OrthographicFrustum from '@cesium/engine/Source/Core/OrthographicFrustum.js';
import OrthographicOffCenterFrustum from '@cesium/engine/Source/Core/OrthographicOffCenterFrustum.js';
import JulianDate from '@cesium/engine/Source/Core/JulianDate.js';
import UniformState from '@cesium/engine/Source/Renderer/UniformState.js';
import AutomaticUniforms from '@cesium/engine/Source/Renderer/AutomaticUniforms.js';
import {suite} from './lib.mjs';
const ref={Color,Cartesian2,Cartesian3,Cartesian4,Matrix2,Matrix3,Matrix4,BoundingRectangle,Ellipsoid,GeographicProjection,WebMercatorProjection,Cartographic,PerspectiveFrustum,PerspectiveOffCenterFrustum,OrthographicFrustum,OrthographicOffCenterFrustum,JulianDate,UniformState,AutomaticUniforms};

export default function rendererCompletion(){
const s=suite('RendererCompletion');
// Select the documented TEME fallback explicitly; fixture generation does not fetch XYS chunks.
const original=Transforms.computeIcrfToCentralBodyFixedMatrix;
Transforms.computeIcrfToCentralBodyFixedMatrix=Transforms.computeTemeToPseudoFixedMatrix;
  const numeric=v=>{
    if(typeof v==='number')return [Math.fround(v)];
    if(v instanceof ref.Color)return [v.red,v.green,v.blue,v.alpha].map(Math.fround);
    if(v instanceof ref.Cartesian2)return [v.x,v.y].map(Math.fround);
    if(v instanceof ref.Cartesian3)return [v.x,v.y,v.z].map(Math.fround);
    if(v instanceof ref.Cartesian4)return [v.x,v.y,v.z,v.w].map(Math.fround);
    if(Array.isArray(v))return v.flatMap(numeric);
    return Array.from({length:v.length??(v instanceof ref.Matrix3?9:16)},(_,i)=>Math.fround(v[i]));
  };
  const frameCases=[{moveCamera:true},{mode:1,moveCamera:true},{}, {light:true},{mode:2,frustum:'orthoOffcenter'}, {mode:1,mercator:true}, {mode:0}, {frustum:'ortho'}, {frustum:'offcenter'}, {frustum:'orthoOffcenter'}, {sphere:true,noCartographic:true}, {noCartographic:true}, {infiniteDistance:true,subfrustum:true}, {date:'2000-01-01T12:00:00Z'}];
  for(const config of frameCases){
   const ellipsoid=config.sphere?ref.Ellipsoid.UNIT_SPHERE:ref.Ellipsoid.WGS84,mode=config.mode??3,cartographic=new ref.Cartographic(.4,.2,1000),position=mode===3?ellipsoid.cartographicToCartesian(cartographic):new ref.Cartesian3(100000,200000,300000),direction=mode===3?ref.Cartesian3.normalize(ref.Cartesian3.negate(position,new ref.Cartesian3()),new ref.Cartesian3()):new ref.Cartesian3(-1,0,0),right=ref.Cartesian3.normalize(ref.Cartesian3.cross(direction,ref.Cartesian3.UNIT_Z,new ref.Cartesian3()),new ref.Cartesian3()),up=ref.Cartesian3.cross(right,direction,new ref.Cartesian3()),view=ref.Matrix4.computeView(position,direction,up,right,new ref.Matrix4());
   const frusta={perspective:new ref.PerspectiveFrustum({fov:1.1,aspectRatio:4/3,near:2,far:12345,xOffset:.2,yOffset:-.1}),ortho:new ref.OrthographicFrustum({width:100,aspectRatio:4/3,near:2,far:12345}),offcenter:new ref.PerspectiveOffCenterFrustum({left:-1,right:2,top:3,bottom:-2,near:2,far:12345}),orthoOffcenter:new ref.OrthographicOffCenterFrustum({left:-50,right:50,top:40,bottom:-40,near:2,far:12345})};
   const frame={mode,mapProjection:new (config.mercator?ref.WebMercatorProjection:ref.GeographicProjection)(ellipsoid),pixelRatio:2,camera:{positionWC:position,directionWC:direction,rightWC:right,upWC:up,positionCartographic:config.noCartographic?undefined:cartographic,viewMatrix:view,inverseViewMatrix:ref.Matrix4.inverseTransformation(view,new ref.Matrix4()),frustum:frusta[config.frustum??'perspective']},time:ref.JulianDate.fromIso8601(config.date??'2025-01-02T03:04:05Z'),context:{defaultCubeMap:{},drawingBufferWidth:800},frameNumber:37,morphTime:.75,fog:{density:.001,visualDensityScalar:2,minimumBrightness:.1},atmosphere:{hueShift:.1,saturationShift:.2,brightnessShift:.3,lightIntensity:4,rayleighCoefficient:new ref.Cartesian3(.001,.002,.003),rayleighScaleHeight:10000,mieCoefficient:new ref.Cartesian3(.004,.005,.006),mieScaleHeight:3000,mieAnisotropy:.8,dynamicLighting:2},invertClassificationColor:new ref.Color(.4,.3,.2,.1),backgroundColor:new ref.Color(.1,.2,.3,.4),splitPosition:.3,maximumScreenSpaceError:8,minimumDisableDepthTestDistance:config.infiniteDistance?Infinity:100,specularEnvironmentMapsMaximumLOD:5,sphericalHarmonicCoefficients:Array.from({length:9},(_,i)=>new ref.Cartesian3(i*.1,i*.2,i*.3))};
   if(config.light)frame.light={direction:new ref.Cartesian3(1,2,3),color:new ref.Color(.3,.6,.9,1),intensity:4};
   const state=new ref.UniformState();state.viewport=new ref.BoundingRectangle(7,9,640,480);state.model=ref.Matrix4.fromArray([2,0,0,0,0,3,0,0,0,0,4,0,12,34,56,1]);state.gamma=2.2;state.update(frame);state.updatePass(8);
   if(config.subfrustum)state.updateFrustum(new ref.PerspectiveOffCenterFrustum({left:-1,right:1,top:1,bottom:-1,near:3,far:100}));
   if(config.moveCamera){const camera=frame.camera;camera.positionWC=ref.Cartesian3.add(camera.positionWC,new ref.Cartesian3(10,20,30),new ref.Cartesian3());camera.positionCartographic=undefined;camera.viewMatrix=ref.Matrix4.computeView(camera.positionWC,camera.directionWC,camera.upWC,camera.rightWC,new ref.Matrix4());camera.inverseViewMatrix=ref.Matrix4.inverseTransformation(camera.viewMatrix,new ref.Matrix4());state.updateCamera(camera);}

   for(const [name,uniform]of Object.entries(ref.AutomaticUniforms)){
    if([35678,35680].includes(uniform._datatype))continue;
    s.add('frameUniform',[config,name],()=>numeric(uniform.getValue(state)));
   }
  }
  Transforms.computeIcrfToCentralBodyFixedMatrix=original;
  return [s];
}
