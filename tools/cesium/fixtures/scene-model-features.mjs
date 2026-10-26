import FeatureTable from '@cesium/engine/Source/Scene/Model/ModelFeatureTable.js';
import ModelType from '@cesium/engine/Source/Scene/Model/ModelType.js';
import PropertyTable from '@cesium/engine/Source/Scene/PropertyTable.js';
import JsonMetadataTable from '@cesium/engine/Source/Scene/JsonMetadataTable.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import {Color} from '@cesium/engine';
import {suite,enc} from './lib.mjs';
export function sceneModelFeatures() {
 const s=suite('ModelFeatures');const previous=ContextLimits._maximumTextureSize;ContextLimits._maximumTextureSize=2;
 function run(c) {
  const model={type:ModelType.GLTF};
  const table=new FeatureTable({model,propertyTable:new PropertyTable({count:3,jsonMetadataTable:new JsonMetadataTable({count:3,properties:{height:[10,20,30],name:['a','b','c']}})})});
  // CPU fixture: GPU upload and pick lifetimes have separate native tests.
  table._batchTexture.update=()=>{};
  const snapshot=()=>({features:Array.from({length:3},(_,i)=>{const f=table.getFeature(i);return {id:f.featureId,show:f.show,color:enc(f.color),ids:f.getPropertyIds(),height:f.getPropertyInherited('height'),name:f.getProperty('name'),added:f.getProperty('added')??null,same:f===table.getFeature(i),owner:f.primitive===model};}),translucent:table.batchTexture.translucentFeaturesLength,commands:table._styleCommandsNeeded,dirty:table.styleCommandsNeededDirty,values:table.batchTexture._batchValues?Array.from(table.batchTexture._batchValues):null,dimensions:enc(table.batchTexture.textureDimensions),step:enc(table.batchTexture.textureStep)});
  const out=[snapshot()];
  for(const op of c.ops) {
   const f=table.getFeature(op.id??0);
   if(op.kind==='show') f.show=op.value;
   else if(op.kind==='color') f.color=new Color(...op.value);
   else if(op.kind==='allShow') table.setAllShow(op.value);
   else if(op.kind==='allColor') table.setAllColor(new Color(...op.value));
   else if(op.kind==='property') f.setProperty(op.name,op.value);
   else if(op.kind==='update') table.update({});
   else if(op.kind==='reset') table.applyStyle(undefined);
   else if(op.kind==='style') table.applyStyle({color:{evaluateColor:f=>f.getProperty('height')>=20?new Color(.2,.4,.6,.5):Color.WHITE},show:{evaluate:f=>f.getProperty('height')!==20}});
   out.push(snapshot());
  }return out;
 }
 for(const ops of [[],[{kind:'show',id:1,value:false},{kind:'color',id:1,value:[1,0,0,.5]},{kind:'show',id:1,value:true}], [{kind:'color',id:0,value:[.1,.2,.3,.4]},{kind:'update'},{kind:'update'},{kind:'reset'},{kind:'update'}], [{kind:'allColor',value:[0,1,0,.5]},{kind:'update'},{kind:'allShow',value:false},{kind:'update'}], [{kind:'style'},{kind:'update'},{kind:'property',id:2,name:'added',value:{hello:[1,2]}},{kind:'property',id:0,name:'height',value:100},{kind:'style'},{kind:'update'}], [{kind:'show',value:true},{kind:'color',value:[1,1,1,1]}]]) s.add('trace',[{ops}],run);
 ContextLimits._maximumTextureSize=previous;return s;
}
