import Stage from '@cesium/engine/Source/Scene/Model/MetadataPipelineStage.js';
import parse from '@cesium/engine/Source/Scene/parseStructuralMetadata.js';
import Schema from '@cesium/engine/Source/Scene/MetadataSchema.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import MC from '@cesium/engine/Source/Scene/ModelComponents.js';
import {resources,primitiveResources,basePrimitive,shaderSnapshot,uv} from './scene-model-stages.mjs';
import {Cartesian3} from '@cesium/engine';
import {suite} from './lib.mjs';
export function sceneModelMetadataStage() {
 const s=suite('ModelMetadataStage');const old=[ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth];ContextLimits._minimumAliasedLineWidth=1;ContextLimits._maximumAliasedLineWidth=1;
 function descriptor(kind,opts={}) {
  const schema={classes:{sensor:{properties:{temperature:{type:'SCALAR',componentType:'FLOAT32',min:-20,max:80,noData:-1,default:0,offset:1,scale:2},code:{type:'SCALAR',componentType:'UINT8',noData:255,default:1},tint:{type:'VEC3',componentType:'UINT8',normalized:true,min:[0,0,0],max:[255,255,255]}}}}};
  const extension={};const attrs=[];
  if(kind==='attribute') {extension.propertyAttributes=[{class:'sensor',properties:{temperature:{attribute:'_TEMP',offset:3,scale:4},tint:{attribute:'_TINT'}}}];attrs.push({name:'_TEMP',type:'SCALAR',componentDatatype:5126},{name:'_TINT',type:'VEC3',componentDatatype:5126});}
  if(kind==='texture') {extension.propertyTextures=[{class:'sensor',properties:{temperature:{index:0,channels:[0,1,2,3]},code:{index:1,channels:[0]},tint:{index:1,channels:[0,1,2]}}}];if(opts.transform) extension.propertyTextures[0].properties.temperature.extensions={KHR_texture_transform:{offset:[.2,.3],scale:[2,3],rotation:.5,texCoord:1}};}
  if(kind==='table') extension.propertyTables=[{class:'sensor',count:2,properties:{temperature:{values:0}}}];
  return {kind,schema,extension,attrs,webgl2:opts.webgl2!==false,stats:!!opts.stats,usageVertex:opts.usageVertex??[],usageFragment:opts.usageFragment??[],noTexture:!!opts.noTexture,textureFeature:!!opts.textureFeature};
 }
 function run(c) {
  const raw=new Uint8Array(new Float32Array([10,20]).buffer);const metadata=parse({extension:c.extension,schema:Schema.fromJson(c.schema),bufferViews:{0:raw},textures:{0:{marker:true},1:{marker:true}}});
  if(c.kind==='table'&&!c.noTexture) metadata.propertyTables[0]._texture={marker:true};
  const statistics=c.stats?{classes:{sensor:{properties:{temperature:{min:-10,max:50,mean:20,median:25,standardDeviation:2,variance:4,sum:100},tint:{min:[0,0,0],max:[1,1,1],mean:[.2,.3,.4]}}}}}:undefined;
  const used=(a)=>Object.fromEntries(a.map(v=>[v,true]));
  const {model,nodeResources}=resources({structuralMetadata:metadata,content:statistics?{tileset:{metadataExtension:{statistics}}}:undefined,customShader:{usedVariablesVertex:{metadataSet:used(c.usageVertex)},usedVariablesFragment:{metadataSet:used(c.usageFragment)}}});
  const primitive=basePrimitive(c.attrs.map(a=>({...a,count:4,constant:a.type==='VEC3'?new Cartesian3(0,0,0):0})));
  primitive.propertyAttributeIds=c.kind==='attribute'?[0]:[];primitive.propertyTextureIds=c.kind==='texture'?[0]:[];
  const f=c.textureFeature?new MC.FeatureIdTexture():new MC.FeatureIdAttribute();Object.assign(f,{propertyTableId:0,positionalLabel:'featureId_0'});if(c.textureFeature) f.textureReader={};primitive.featureIds=[f];
  const p=primitiveResources(nodeResources,primitive);Stage.process(p,primitive,{context:{webgl2:c.webgl2}});
  return {...shaderSnapshot(p),uniforms:Object.fromEntries(Object.entries(p.uniformMap).map(([k,f])=>{const v=f();return [k,v?.marker?null:uv(v)];}))};
 }
 for(const kind of ['empty','attribute','texture','table']) for(const webgl2 of [false,true]) for(const stats of [false,true]) {if(kind==='texture'&&!webgl2)continue;const c=descriptor(kind,{webgl2,stats,transform:true,usageVertex:['temperature'],usageFragment:['code','tint']});s.add('stage',[c],run);}
 for(const opts of [{},{usageVertex:['temperature']},{usageFragment:['temperature'],noTexture:true},{usageVertex:['temperature'],usageFragment:['temperature'],textureFeature:true}]) s.add('stage',[descriptor('table',opts)],run);
 // Cesium 23 emits invalid unsigned GLSL in its WebGL1 property-texture path.
 // Those cases are tested against the corrected shader contract in Rust rather
 // than treating the upstream invalid output as a parity oracle.
 for(const stats of [false,true]) s.add('stage',[descriptor('texture',{stats})],run);
 for(const usageFragment of [[],['temperature','code','tint']]) s.add('stage',[descriptor('table',{stats:true,usageVertex:['temperature','code','tint'],usageFragment})],run);
 // Integer value transforms also need typed arithmetic: the pinned helper only
 // provides float overloads. Exact unsigned transform behavior has native coverage.
 s.add('stage',[descriptor('table',{usageFragment:['code']})],run);
 [ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth]=old;
 if(s.cases.some(c=>c.r.throws))throw Error(JSON.stringify(s.cases.filter(c=>c.r.throws)));return s;
}
