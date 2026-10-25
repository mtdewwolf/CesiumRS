import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
// Stage outputs from the pinned CesiumJS implementation; texture handles are
// represented by null, while sampler declarations/providers remain observable.
import {resources, primitiveResources, basePrimitive, shaderSnapshot, uv, frameState} from './scene-model-stages.mjs';
import Geometry from '@cesium/engine/Source/Scene/Model/GeometryPipelineStage.js';
import Feature from '@cesium/engine/Source/Scene/Model/FeatureIdPipelineStage.js';
import Selected from '@cesium/engine/Source/Scene/Model/SelectedFeatureIdPipelineStage.js';
import MetadataPicking from '@cesium/engine/Source/Scene/Model/MetadataPickingPipelineStage.js';
import CPUStyling from '@cesium/engine/Source/Scene/Model/CPUStylingPipelineStage.js';
import Batch from '@cesium/engine/Source/Scene/Model/BatchTexturePipelineStage.js';
import Tileset from '@cesium/engine/Source/Scene/Model/TilesetPipelineStage.js';
import Material from '@cesium/engine/Source/Scene/Model/MaterialPipelineStage.js';
import Planes from '@cesium/engine/Source/Scene/Model/ModelClippingPlanesPipelineStage.js';
import Polygons from '@cesium/engine/Source/Scene/Model/ModelClippingPolygonsPipelineStage.js';
import MC from '@cesium/engine/Source/Scene/ModelComponents.js';
import {Cartesian2, Cartesian3, Cartesian4, Color, Matrix4, ClippingPlane, ClippingPlaneCollection} from '@cesium/engine';
import {suite} from './lib.mjs';

function result(r) {
 const uniforms = Object.fromEntries(Object.entries(r.uniformMap).map(([k,f])=>[k, uv(f())]));
 return {...shaderSnapshot(r), uniforms};
}
export function sceneModelNextStages() {
 const s = suite('ModelNextStages');
 const oldLimits=[ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth,ContextLimits._maximumTextureSize];
 ContextLimits._maximumTextureSize=4096;
 ContextLimits._minimumAliasedLineWidth=1;ContextLimits._maximumAliasedLineWidth=1;
 function run(c) {
  const {model, modelResources: m, nodeResources: n} = resources({
   featureIdLabel:'buildings', instanceFeatureIdLabel:'instances', backFaceCulling:!c.noCull,
   pointCloudShading:{normalShading:c.normalShading}, color:c.color ? new Color(1,1,1,1):undefined,
   colorBlendMode:0, colorBlendAmount:.5, hasSkipLevelOfDetail:()=>c.skip,
   _clippingPlanesMatrix:Matrix4.IDENTITY,
  });
  const primitive = basePrimitive(c.normals ? [{semantic:'NORMAL',type:'VEC3',constant:new Cartesian3(0,0,1)}]:[]);
  primitive.material = new MC.Material();
  const p = primitiveResources(n, primitive);
  if(c.stage==='feature' || c.stage==='selected') {
   Geometry.process(p, primitive, frameState());
   const f = c.texture ? new MC.FeatureIdTexture() : new MC.FeatureIdAttribute();
   Object.assign(f,{featureCount:4, propertyTableId:0, setIndex:0,label:'buildings',positionalLabel:'featureId_0',nullFeatureId:c.nullId});
   if(c.texture) f.textureReader={texCoord:0,channels:'rg'};
   primitive.featureIds=[f];
   if(c.stage==='feature') Feature.process(p, primitive, frameState());
   else Selected.process(p, primitive, frameState());
  } else if(c.stage==='metadata') MetadataPicking.process(p, primitive, frameState());
  else if(c.stage==='cpu') CPUStyling.process(p, primitive, frameState());
  else if(c.stage==='batch') {
   model.featureTableId=0; model.featureTables=[{featuresLength:4,batchTexture:{textureStep:new Cartesian4(.1,.2,.3,.4),textureDimensions:new Cartesian2(4,c.rows)}}];
   Batch.process(p, primitive, frameState());
  } else if(c.stage==='tileset') Tileset.process(p, model, frameState());
  else if(c.stage==='material') {
   const mat=primitive.material;
   mat.unlit=!!c.unlit; mat.doubleSided=!!c.doubleSided;
   mat.alphaMode=c.alphaMode??'OPAQUE';mat.alphaCutoff=.3;
   mat.pointDiameter=c.diameter;
   mat.metallicRoughness.metallicFactor=.4;mat.metallicRoughness.roughnessFactor=.7;
   mat.metallicRoughness.baseColorFactor=new Cartesian4(.2,.3,.4,.5);
   if(c.specular) {mat.specular=new MC.Specular();mat.specular.specularFactor=.5;}
   if(c.anisotropy) {mat.anisotropy=new MC.Anisotropy();mat.anisotropy.anisotropyStrength=.6;mat.anisotropy.anisotropyRotation=.7;}
   if(c.clearcoat) {mat.clearcoat=new MC.Clearcoat();mat.clearcoat.clearcoatFactor=.8;mat.clearcoat.clearcoatRoughnessFactor=.9;}
   Material.process(p, primitive, {...frameState(),pixelRatio:2});
  } else if(c.stage==='planes') {
   model.clippingPlanes=new ClippingPlaneCollection({planes:[new ClippingPlane(Cartesian3.UNIT_X,0)],edgeColor:new Color(.2,.3,.4,.5),edgeWidth:3,unionClippingRegions:c.union});
   Planes.process(m,model,{context:{floatingPointTexture:c.float,maximumTextureSize:4096}});
   return result(m);
  } else if(c.stage==='polygons') {
   model.clippingPolygons={inverse:c.inverse,extentsCount:2};
   Polygons.process(m,model,frameState());return result(m);
  }
  return result(p);
 }
 for(const stage of ['metadata','cpu','tileset']) for(const v of [false,true]) s.add('stage',[{stage,color:v,skip:v}],run);
 for(const rows of [1,2]) s.add('stage',[{stage:'batch',rows}],run);
 for(const texture of [false,true]) for(const nullId of [undefined,255]) s.add('stage',[{stage:'selected',texture,nullId}],run);
 for(const texture of [false,true]) s.add('stage',[{stage:'feature',texture}],run);
 for(const c of [{},{normals:true},{normals:true,unlit:true},{normals:true,doubleSided:true,alphaMode:'BLEND'},{normals:true,alphaMode:'MASK',diameter:4},{normals:true,specular:true,anisotropy:true,clearcoat:true},{normals:true,noCull:true},{normals:true,normalShading:false}]) s.add('stage',[{stage:'material',...c}],run);
 for(const float of [false,true]) for(const union of [false,true]) s.add('stage',[{stage:'planes',float,union}],run);
 for(const inverse of [false,true]) s.add('stage',[{stage:'polygons',inverse}],run);
 [ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth,ContextLimits._maximumTextureSize]=oldLimits;
 if(s.cases.some(c=>c.r.throws)) throw new Error('Unexpected stage fixture exception: '+JSON.stringify(s.cases.filter(c=>c.r.throws)));
 return s;
}
