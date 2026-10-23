import {Color,Cartesian3} from '@cesium/engine';
import OIT from '@cesium/engine/Source/Scene/OIT.js';
import View from '@cesium/engine/Source/Scene/View.js';
import PickDepth from '@cesium/engine/Source/Scene/PickDepth.js';
import DrawCommand from '@cesium/engine/Source/Renderer/DrawCommand.js';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import RenderState from '@cesium/engine/Source/Renderer/RenderState.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import BoundingRectangle from '@cesium/engine/Source/Core/BoundingRectangle.js';
import {scene} from './scene-transforms.mjs';
import {suite,enc,makeRng} from './lib.mjs';
export default function sceneView(){
  const oldLimits=[ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth];ContextLimits._minimumAliasedLineWidth=ContextLimits._maximumAliasedLineWidth=1;
  const oit=suite('OIT'),view=suite('ViewResources'),depth=suite('PickDepth');
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
  return [oit,view,depth];
}
