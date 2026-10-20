// Portable renderer contracts, always evaluated by the pinned CesiumJS engine.
import BufferUsage from '@cesium/engine/Source/Renderer/BufferUsage.js';
import TextureWrap from '@cesium/engine/Source/Renderer/TextureWrap.js';
import TextureMinificationFilter from '@cesium/engine/Source/Renderer/TextureMinificationFilter.js';
import TextureMagnificationFilter from '@cesium/engine/Source/Renderer/TextureMagnificationFilter.js';
import MipmapHint from '@cesium/engine/Source/Renderer/MipmapHint.js';
import Pass from '@cesium/engine/Source/Renderer/Pass.js';
import Sampler from '@cesium/engine/Source/Renderer/Sampler.js';
import ShaderFunction from '@cesium/engine/Source/Renderer/ShaderFunction.js';
import ShaderStruct from '@cesium/engine/Source/Renderer/ShaderStruct.js';
import ShaderDestination from '@cesium/engine/Source/Renderer/ShaderDestination.js';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import RenderState from '@cesium/engine/Source/Renderer/RenderState.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import {suite} from './lib.mjs';
export default function rendererFoundations() {
 const out=[],add=name=>{const s=suite(name);out.push(s);return s;};
 for(const [name,e] of Object.entries({BufferUsage,TextureWrap,TextureMinificationFilter,TextureMagnificationFilter,MipmapHint})) {
  const s=add(name);s.add('constants',[],()=>Object.fromEntries(Object.entries(e).filter(([,v])=>typeof v==='number')));
  for(const v of [-1,0,1,2,3,5121,9728,9729,9984,9985,9986,9987,10497,33071,33648,35040,35044,35048,35049,4352,4353,4354,999999])s.add('validate',[v],v=>e.validate(v));
 }
 add('Pass').add('constants',[],()=>Pass);
 const sampler=add('Sampler');
 for(const opts of [{},{wrapR:10497,wrapS:33648,wrapT:10497},{minificationFilter:9987,magnificationFilter:9728,maximumAnisotropy:4},{maximumAnisotropy:1.5}])sampler.add('values',[opts],opts=>{const s=new Sampler(opts);return [s.wrapR,s.wrapS,s.wrapT,s.minificationFilter,s.magnificationFilter,s.maximumAnisotropy,Sampler.equals(s,new Sampler(opts)),Sampler.equals(s,Sampler.NEAREST)];});
 const fn=add('ShaderFunction');
 for(const sig of ['void main()','vec3 compute(vec3 normal)','float f()'])for(const body of [[],['return;'],['vec3 n = vec3(0.0);','return n;']])fn.add('lines',[sig,body],(sig,body)=>{const f=new ShaderFunction(sig);f.addLines(body);return f.generateGlslLines();});
 const struct=add('ShaderStruct');
 for(const name of ['Attributes','Empty','Test'])for(const fields of [[],[['vec3','position']],[['vec3','normal'],['float','weight'],['mat4','transform']]])struct.add('lines',[name,fields],(name,fields)=>{const s=new ShaderStruct(name);for(const [t,n]of fields)s.addField(t,n);return s.generateGlslLines();});
 const dest=add('ShaderDestination');
 for(let a=0;a<4;a++)for(let b=0;b<4;b++)dest.add('flags',[a,b],(a,b)=>[ShaderDestination.includesVertexShader(a),ShaderDestination.includesFragmentShader(a),ShaderDestination.union(a,b),ShaderDestination.intersection(a,b)]);
 const source=add('ShaderSource');
 for(const text of ['void main() {}','void main(void) { out_FragColor = vec4(1.0); }','void   main ( ) { }\nvoid main(void) {}','// void main() {}\nvoid main() {}'])source.add('replaceMain',[text,'renamed'],(text,name)=>ShaderSource.replaceMain(text,name));
 const sources=[['void main() { out_FragColor = vec4(1.0); }'],['#version 300 es\nprecision highp float;\n// comment\nvoid main(void) {}'],['/**\n * docs\n */\nfloat x;','void main() { gl_Position = vec4(x); }'],['layout(location = 0) out vec4 out_FragColor;\nvoid main() {out_FragColor=vec4(0.); }']];
 for(const sources0 of sources)for(const fragment of [false,true])for(const floats of [false,true])source.add('combine',[sources0,['TEST 1',''],fragment,floats],(sources,defines,fragment,floats)=>{const s=new ShaderSource({sources,defines,includeBuiltIns:false}),c={webgl2:true,textureFloatLinear:floats,floatingPointTexture:floats};return fragment?s.createCombinedFragmentShader(c):s.createCombinedVertexShader(c);});
 ContextLimits._minimumAliasedLineWidth=1;ContextLimits._maximumAliasedLineWidth=8;ContextLimits._maximumViewportWidth=16384;ContextLimits._maximumViewportHeight=16384;
 const state=add('RenderState');
 const options=[{}, {depthTest:{enabled:true},cull:{enabled:true}},{viewport:{x:1,y:2,width:8,height:9},colorMask:{red:false,alpha:false}},{blending:{enabled:true,functionSourceRgb:770,functionDestinationRgb:771},depthMask:false},{stencilTest:{enabled:true,reference:3,mask:255,frontOperation:{zPass:7681}}},{scissorTest:{enabled:true,rectangle:{x:2,y:1,width:3,height:4}},sampleCoverage:{enabled:true,value:.5,invert:true}},{frontFace:2304,lineWidth:2,polygonOffset:{enabled:true,factor:1.25,units:2},depthRange:{near:.1,far:.9}},{stencilMask:15,blending:{color:{red:.1,blue:.5},equationAlpha:32779}}];
 for(const opt of options)state.add('defaults',[opt],opt=>{const s=RenderState.getState(RenderState.fromCache(opt));delete s.id;return s;});
 return out;
}
