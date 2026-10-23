// Executes the actual upstream framebuffer/depth modules on a real GPU.
export async function run(wasm,ref,canvas,attributes) {
  const passed=[];
  for(const webgl1 of [false,true])for(const hdr of [false,true])for(const samples of [1,4])for(const picking of [false,true])for(const scissor of [false,true])for(const stencil of [false,true]) {
    const config={webgl1,hdr,samples,picking,scissor,stencil,depth:stencil?.2:.7,clear:stencil};
    window.lastSceneCase={depthResources:true,...config};
    const gl=canvas().getContext(webgl1?'webgl':'webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes,requestWebgl1:webgl1});
    const viewport=new ref.BoundingRectangle(0,0,8,8),pass=new ref.PassState(context);pass.viewport=scissor?new ref.BoundingRectangle(1,2,5,4):viewport;
    const read=framebuffer=>{
      const pixels=context.readPixels({width:8,height:8,framebuffer});
      // Cesium returns raw binary16 for HALF_FLOAT; the portable renderer
      // returns the guaranteed RGBA/FLOAT readback. Compare numeric values.
      return Array.from(pixels,v=>{if(!(pixels instanceof Uint16Array))return v;const sign=v&32768?-1:1,exponent=(v>>10)&31,mantissa=v&1023;return sign*(exponent===0?mantissa*2**-24:exponent===31?(mantissa?NaN:Infinity):(1+mantissa/1024)*2**(exponent-15));});
    };
    const scene=new ref.SceneFramebuffer();scene.update(context,viewport,hdr,samples);scene.clear(context,pass,ref.Color.RED);scene.prepareColorTextures(context);
    const cleared=read(scene._colorFramebuffer._framebuffer);
    const globe=new ref.GlobeDepth();globe.picking=picking;globe.update(context,pass,viewport,samples,hdr,config.clear);
    new ref.ClearCommand({framebuffer:globe.framebuffer,color:new ref.Color(.2,.4,.6,1),depth:.3,stencil:128}).execute(context,pass);
    globe.executeCopyDepth(context,pass);const packed=read(globe._copyDepthFramebuffer.framebuffer);
    const other=new ref.SceneFramebuffer();other.update(context,viewport,false,1);
    const otherPass=new ref.PassState(context);otherPass.viewport=pass.viewport;otherPass.framebuffer=other.framebuffer;
    new ref.ClearCommand({color:ref.Color.TRANSPARENT,depth:config.depth,stencil:stencil?128:0}).execute(context,otherPass);
    globe.executeUpdateDepth(context,otherPass,other.depthStencilTexture);const merged=read(globe._copyDepthFramebuffer.framebuffer);
    const pick=new ref.PickDepth();pick.update(context,other.depthStencilTexture);pick.executeCopyDepth(context,pass);
    const picked=read(pick.framebuffer),depth=pick.getDepth(context,4,4);
    const translucent=new ref.GlobeTranslucencyFramebuffer();translucent.updateAndClear(hdr,viewport,context,pass);translucent.clearClassification(context,pass);
    new ref.ClearCommand({framebuffer:translucent.classificationFramebuffer,color:new ref.Color(1,0,0,.5),depth:.4}).execute(context,pass);translucent.packDepth(context,pass);
    const translucentDepth=read(translucent.packedDepthFramebuffer);
    const edge=new ref.EdgeFramebuffer();edge.update(context,viewport,hdr,globe.colorFramebufferManager.getColorTexture(0),globe.depthStencilTexture);edge.clear(context,pass,ref.Color.LIME);
    const edgePixels=read(edge.framebuffer);
    const expected={cleared,packed,merged,picked,depth,translucent:translucentDepth,edge:edgePixels},actual=JSON.parse(wasm.scene_depth_resources(gl,JSON.stringify(config)));
    if(JSON.stringify(actual)!==JSON.stringify(expected)||gl.getError()!==0){
      const differences=Object.entries(expected).map(([key,e])=>({key,difference:Array.isArray(e)?e.flatMap((v,i)=>v===actual[key][i]?[]:[[i,actual[key][i],v]]).slice(0,12):[actual[key],e]}));
      throw Error(JSON.stringify({config,differences,glError:gl.getError()}));
    }
    edge.destroy();pick.destroy();translucent.destroy();globe.destroy();other.destroy();scene.destroy();context.destroy();passed.push({depthResources:true,...config});
  }
  return passed;
}
