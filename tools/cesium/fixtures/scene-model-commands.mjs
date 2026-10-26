import { BoundingSphere, Cartesian3, Color, Matrix4, GeographicProjection, Resource, Ray } from "@cesium/engine";
import DrawCommand from "@cesium/engine/Source/Renderer/DrawCommand.js";
import RenderState from "@cesium/engine/Source/Renderer/RenderState.js";
import Pass from "@cesium/engine/Source/Renderer/Pass.js";
import ContextLimits from "@cesium/engine/Source/Renderer/ContextLimits.js";
import ModelDrawCommand from "@cesium/engine/Source/Scene/Model/ModelDrawCommand.js";
import ClassificationModelDrawCommand from "@cesium/engine/Source/Scene/Model/ClassificationModelDrawCommand.js";
import Model from "@cesium/engine/Source/Scene/Model/Model.js";
import pickModel from "@cesium/engine/Source/Scene/Model/pickModel.js";
import { suite, enc } from "./lib.mjs";

function snapshot(c) {
  const r = c.renderState;
  return { pass: c.pass, count: c.count ?? null, offset: c.offset ?? 0, matrix: enc(c.modelMatrix), sphere: enc(c.boundingVolume),
    cull: r.cull, depthMask: r.depthMask, depthTest: r.depthTest, colorMask: r.colorMask, stencilMask: r.stencilMask,
    stencil: r.stencilTest, polygonOffset: r.polygonOffset, blending: r.blending.enabled,
    cast: c.castShadows, receive: c.receiveShadows, debug: c.debugShowBoundingVolume,
    pickOnly: c.pickOnly, cullCommand: c.cull, pickId: c.pickId ?? null };
}
function command() {
  return new DrawCommand({ modelMatrix: Matrix4.clone(Matrix4.IDENTITY), boundingVolume: new BoundingSphere(Cartesian3.ZERO, 1),
    renderState: RenderState.fromCache({ cull: {enabled: true}, depthTest: {enabled: true} }), pass: Pass.OPAQUE,
    count: 12, primitiveType: 4, castShadows: true, receiveShadows: true, debugShowBoundingVolume: false, cull: true, uniformMap:{} });
}
export function sceneModelCommands() {
  const limits=[ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth];
  ContextLimits._minimumAliasedLineWidth=1;ContextLimits._maximumAliasedLineWidth=1;
  const draw = suite("ModelDrawCommand");
  const configurations = [];
  for (const translucent of [false, true]) for (const doubleSided of [false,true]) for (const style of [null, 0, 1, 2]) configurations.push({translucent,doubleSided,style});
  for (const silhouetteAlpha of [1, .4]) for (const invisible of [false,true]) for (const pick of [false,true]) configurations.push({silhouette:true,silhouetteAlpha,invisible,pick});
  for (const mixed of [false,true]) for (const final of [false,true]) for (const depth of [0,1,3,7]) configurations.push({skip:true,mixed,final,depth});
  configurations.push({mode:2,idl:1}, {mode:2,idl:-1}, {mode:2,idl:1,project:true}, {mode:1}, {mode:2});
  for (const shadows of [0,1,2,3]) configurations.push({shadows,cull:false,debug:true,translate:[1,2,3]});
  for (const config of configurations) draw.add("commands", [config], (c) => {
    const base = command(); if (c.translucent) base.pass = Pass.TRANSLUCENT;
    const model = { shadows: 1, styleCommandsNeeded: c.style ?? undefined, _silhouetteId: 260, silhouetteColor: new Color(1,0,0,c.silhouetteAlpha??1),
      isInvisible:()=>c.invisible??false, _projectTo2D:c.project??false,
      sceneGraph:{_boundingSphere2D: new BoundingSphere(new Cartesian3(0,(c.idl??0)*Math.PI*6378137,0), 10)},
      content:{tileset:{hasMixedContent:c.mixed??false,_backfaceCommands:[]},tile:{_finalResolution:c.final??false,_selectionDepth:c.depth??0}} };
    if (c.idl) base.modelMatrix[13]=c.idl*Math.PI*6378137;
    const primitive={primitive:{material:{doubleSided:c.doubleSided??false}},boundingSphere:new BoundingSphere(Cartesian3.ZERO,1)};
    const d=new ModelDrawCommand({command:base,primitiveRenderResources:{model,runtimePrimitive:primitive,hasSilhouette:c.silhouette??false,hasSkipLevelOfDetail:c.skip??false}});
    if(c.translate)d.modelMatrix=Matrix4.fromTranslation(Cartesian3.fromArray(c.translate));
    if(c.shadows!==undefined)d.shadows=c.shadows;
    if(c.cull!==undefined)d.backFaceCulling=c.cull;
    if(c.debug!==undefined)d.debugShowBoundingVolume=c.debug;
    const frame={mode:c.mode??3,mapProjection:new GeographicProjection(),passes:{pick:c.pick??false}};
    const commands=[]; d.pushCommands(frame,commands);
    const silhouettes=[]; if(c.silhouette&&!c.pick)d.pushSilhouetteCommands(frame,silhouettes);
    return {commands:commands.map(snapshot),backfaces:model.content.tileset._backfaceCommands.map(snapshot),silhouettes:silhouettes.map(snapshot)};
  });
  const classification=suite("ClassificationModelDrawCommand");
  for(const type of [0,1,2])for(const render of [false,true])for(const pick of [false,true])for(const allow of [false,true])for(const invert of [false,true]) {
    const c={type,render,pick,allow,invert};
    classification.add("commands",[c],(c)=>{
      const primitive={batchLengths:[3,9],batchOffsets:[0,3],boundingSphere:new BoundingSphere(Cartesian3.ZERO,1)};
      const model={classificationType:c.type,allowPicking:c.allow,_enableDebugWireframe:false,debugWireframe:false};
      const d=new ClassificationModelDrawCommand({command:command(),primitiveRenderResources:{model,runtimePrimitive:primitive,pickId:"u_pickColor"}});
      const out=[];d.pushCommands({passes:{render:c.render,pick:c.pick},invertClassification:c.invert},out);return out.map(snapshot);
    });
  }
  const properties=suite("ModelProperties");
  for(const alpha of [null,0,.3,1])for(const classification of [null,0,1,2])for(const stencil of [false,true])for(const size of [0,2]) {
    properties.add("visibility",[{alpha,classification,stencil,size}],(c)=>{
      const m=new Model({loader:{},resource:new Resource({url:"test.gltf"}),color:c.alpha===null?undefined:new Color(1,1,1,c.alpha),classificationType:c.classification??undefined,silhouetteSize:c.size});
      return [m.isTranslucent(),m.isInvisible(),m.hasSilhouette({context:{stencilBuffer:c.stencil}})];
    });
  }
  const pick=suite("pickModel");
  for(const translation of [[0,0,0],[10,20,3]])for(const backface of [false,true])for(const origin of [[0,0,5],[0,0,-5],[3,0,5]])for(const quantized of [false,true]) {
    const c={translation,backface,origin,quantized};
    pick.add("triangle",[c],(c)=>{
      const attribute={semantic:"POSITION",type:"VEC3",componentDatatype:5126,count:3,byteOffset:0,typedArray:new Float32Array([-1,-1,0,1,-1,0,0,1,0])};
      if(c.quantized)attribute.quantization={octEncoded:false,type:"VEC3",componentDatatype:5126,quantizedVolumeStepSize:new Cartesian3(2,2,2),quantizedVolumeOffset:new Cartesian3(0,0,1)};
      const primitive={attributes:[attribute],indices:{typedArray:new Uint16Array([0,1,2])}};
      const computed=Matrix4.fromTranslation(Cartesian3.fromArray(c.translation));
      const graph={computedModelMatrix:computed,_runtimeNodes:[{node:{},computedTransform:Matrix4.IDENTITY,runtimePrimitives:[{primitive}]}]};
      const model={_ready:true,sceneGraph:graph,backFaceCulling:c.backface};
      const origin=Cartesian3.add(Cartesian3.fromArray(c.origin),Cartesian3.fromArray(c.translation),new Cartesian3());
      const direction=new Cartesian3(0,0,c.origin[2]>0?-1:1);
      return pickModel(model,new Ray(origin,direction),{mode:3,context:{webgl2:true}},1,0);
    });
  }
  [ContextLimits._minimumAliasedLineWidth,ContextLimits._maximumAliasedLineWidth]=limits;
  return [draw,classification,properties,pick];
}
