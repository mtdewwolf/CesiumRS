import {Color,Cartesian3,Cartesian2,Matrix4,Math as CesiumMath,Cartographic,Ellipsoid} from '@cesium/engine';
import OIT from '@cesium/engine/Source/Scene/OIT.js';
import View from '@cesium/engine/Source/Scene/View.js';
import FrustumCommands from '@cesium/engine/Source/Scene/FrustumCommands.js';
import Camera from '@cesium/engine/Source/Scene/Camera.js';
import SceneMode from '@cesium/engine/Source/Scene/SceneMode.js';
import Pass from '@cesium/engine/Source/Renderer/Pass.js';
import PickDepth from '@cesium/engine/Source/Scene/PickDepth.js';
import DrawCommand from '@cesium/engine/Source/Renderer/DrawCommand.js';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import RenderState from '@cesium/engine/Source/Renderer/RenderState.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import BoundingRectangle from '@cesium/engine/Source/Core/BoundingRectangle.js';
import defined from '@cesium/engine/Source/Core/defined.js';
import {readFileSync} from 'node:fs';
import {scene} from './scene-transforms.mjs';
import {suite,enc,makeRng} from './lib.mjs';

// Execute the pinned View private helpers unchanged; inject only their imports.
const viewSource=readFileSync(new URL('../node_modules/@cesium/engine/Source/Scene/View.js',import.meta.url),'utf8');
function slice(from,to){
  const start=viewSource.indexOf(from),end=viewSource.indexOf(to,start);
  if(start<0||end<0)throw new Error(`Cannot slice ${from}`);
  return viewSource.slice(start,end);
}
const viewHelpers=new Function('Cartesian3','Matrix4','CesiumMath','FrustumCommands','SceneMode','defined',
  `${slice('const scratchPosition0','function cameraEqual')}${slice('function cameraEqual','/**\n * Check if the camera position')}${slice('function updateFrustums','/**\n * Insert a command')}${slice('function insertIntoBin','const scratchCullingVolume')}\nreturn {cameraEqual,updateFrustums,insertIntoBin};`
)(Cartesian3,Matrix4,CesiumMath,FrustumCommands,SceneMode,defined);

export default function sceneView(){
  const oldLimits=[ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth];ContextLimits._minimumAliasedLineWidth=ContextLimits._maximumAliasedLineWidth=1;
  const oit=suite('OIT'),view=suite('ViewResources'),depth=suite('PickDepth');
  const fitting=suite('View'), binning=suite('ViewBins');
  for(const mode of [0,1,2,3])for(const log of [false,true])for(const ortho of mode===2?[true]:[false,true])for(const near of [1,25,700])for(const far of [near+1,5e4,1e9])for(const step of mode===2?[1.75e6,4e6]:[1.75e6]){
    const o={mode,log,ortho,position:[mode===2?step:0,0,2e7]};
    fitting.add('frustums',[o,near,far,step],(o,near,far,step)=>{
      const s=scene(o),camera=s.camera;
      const v={frustumCommandsList:[]};
      viewHelpers.updateFrustums(v,{mode:o.mode,nearToFarDistance2D:step,farToNearRatio:1000,logarithmicDepthFarToNearRatio:1e9,frameState:{camera,useLogDepth:o.log}},near,far);
      return {cameraZ:camera.position.z,frustums:v.frustumCommandsList.map(f=>({near:f.near,far:f.far,indices:f.indices.slice(),counts:f.commands.map(c=>c.length)}))};
    });
  }
  for(const debug of [false,true])for(const count of [1,2,4])for(const pass of [Pass.OPAQUE,Pass.TRANSLUCENT,Pass.GLOBE])for(const command of [[0,0,false],[1,4,false],[2,3,true],[3,1,false]])
    binning.add('insert',[debug,count,pass,command],(debug,count,pass,command)=>{
      const [near,far,closest]=command;
      const v={frustumCommandsList:Array.from({length:count},(_,i)=>new FrustumCommands(i+1,i+1+count*2)),debugFrustumStatistics:{totalCommands:0,commandsInFrustums:{}}};
      const c={pass,executeInClosestFrustum:closest};
      viewHelpers.insertIntoBin(v,{debugShowFrustums:debug,updateDerivedCommands(){}},{command:c,near,far});
      return {indices:v.frustumCommandsList.map(f=>f.indices.slice()),counts:v.frustumCommandsList.map(f=>f.commands.map(x=>x.length)),overlap:c.debugOverlappingFrustums??null,statistics:debug?v.debugFrustumStatistics:null};
    });
  for(const epsilon of [CesiumMath.EPSILON15,1e-6])for(const delta of [0,1e-16,1e-10,1])for(const component of ['position','direction','up','right','frustum'])
    fitting.add('cameraEqual',[epsilon,delta,component],(epsilon,delta,component)=>{
      const s=scene({mode:3,position:[0,0,2e7]}),a=s.camera,b=Camera.clone(s.camera);
      if(component==='position')b.position.x+=delta;else if(component==='direction')b.direction.x+=delta;else if(component==='up')b.up.x+=delta;else if(component==='right')b.right.x+=delta;else b.frustum.near+=delta;
      return viewHelpers.cameraEqual(a,b,epsilon);
    });
  const inputs=[{sources:['void main(){out_FragColor=vec4(1.0);}']},{sources:['void main(void){if(true){discard;}out_FragColor=czm_phong(vec3(1.0),true,vec4(1.0));}'],defines:['HDR']},{sources:['layout (location = 0) out vec4 out_FragColor;\nvoid main(){out_FragColor=vec4(0.5);}'],includeBuiltIns:false}];
  for(const input of inputs)for(const mrt of [false,true])for(const depthMask of [false,true])oit.add('derive',[input,mrt,depthMask],(input,mrt,depthMask)=>{
    const context={colorBufferFloat:true,depthTexture:true,floatBlend:true,drawBuffers:mrt,shaderCache:{getDerivedShaderProgram:()=>undefined,createDerivedShaderProgram:(_,key,options)=>({...options,id:2,_attributeLocations:options.attributeLocations})}};
    const o=new OIT(context),command=new DrawCommand({shaderProgram:{id:1,vertexShaderSource:new ShaderSource({sources:['void main(){gl_Position=vec4(1.0);}']}),fragmentShaderSource:new ShaderSource(input),_attributeLocations:{position:0}},renderState:RenderState.fromCache({depthMask})});
    const result=o.createDerivedCommands(command,context);
    const encode=c=>({source:{sources:c.shaderProgram.fragmentShaderSource.sources,defines:c.shaderProgram.fragmentShaderSource.defines,includeBuiltIns:c.shaderProgram.fragmentShaderSource.includeBuiltIns},state:{depthMask:c.renderState.depthMask,blending:c.renderState.blending}});
    return [encode(result.translucentCommand),...(result.alphaCommand?[encode(result.alphaCommand)]:[])];
  });
  for(const enabled of [false,true])for(const useOit of [false,true])view.add('lifetime',[enabled,useOit],(enabled,useOit)=>{
    const s=scene({mode:3,position:[0,0,2e7]});s.context={depthTexture:enabled,colorBufferFloat:false,floatBlend:false,drawBuffers:false};s._useOIT=useOit;
    const v=new View(s,s.camera,new BoundingRectangle(0,0,800,600));
    const owned=[v.pickFramebuffer,v.pickDepthFramebuffer,v.sceneFramebuffer,v.edgeFramebuffer,v.globeDepth,v.globeTranslucencyFramebuffer,v.oit,v.translucentTileClassification].filter(Boolean);
    const result={globe:!!v.globeDepth,oit:!!v.oit,classificationSupported:v.translucentTileClassification.isSupported()};
    v.destroy();return {...result,destroyed:owned.map(o=>o.isDestroyed())};
  });
  const rng=makeRng(0xa8bc1);
  for(const pixels of [[0,0,0,0],[255,255,255,255],[255,0,0,0],[0,255,0,0],...Array.from({length:32},()=>Array.from({length:4},()=>Math.floor(rng()*256)))])depth.add('decode',[pixels],pixels=>PickDepth.prototype.getDepth.call({framebuffer:{}},{readPixels:()=>new Uint8Array(pixels)},4,4));
  [ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth]=oldLimits;
  return [oit,view,depth,fitting,binning];
}
