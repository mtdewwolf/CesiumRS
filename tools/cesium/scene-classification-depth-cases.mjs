// A classification-pass probe samples the same packed depth used by the actual
// classification shaders. This isolates Scene's globe/tile depth handoff.
export async function run(wasm,ref,canvas,attributes) {
  const passed=[];
  for(const samples of [1,4])for(const tile of [false,true])for(const log of [false,true])for(const hdr of [false,true]) {
    const config={classify:true,samples,tile,log,hdr};window.lastSceneCase=config;
    const gl=canvas().getContext('webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes});
    context.uniformState._frameState={mode:3};context.uniformState._mode=3;
    context.uniformState.updateFrustum(new ref.PerspectiveFrustum({fov:Math.PI/3,aspectRatio:1,near:700,far:1313}));
    const viewport=new ref.BoundingRectangle(0,0,8,8),pass=new ref.PassState(context);pass.viewport=viewport;
    const globe=new ref.GlobeDepth();globe.update(context,pass,viewport,samples,hdr,false);pass.framebuffer=globe.framebuffer;
    new ref.ClearCommand({color:ref.Color.BLACK,depth:1,stencil:0}).execute(context,pass);
    if(tile)globe.executeCopyDepth(context,pass);
    const program=ref.ShaderProgram.fromCache({context,vertexShaderSource:'in vec4 position;\nvoid main(){gl_Position=czm_projection*vec4(position.xy*.65*1100.0*.5,-1100.0,1.0);}',fragmentShaderSource:'void main(){out_FragColor=vec4(.2,.3,.4,1.0);}',attributeLocations:{position:0}});
    let command=context.createViewportQuadCommand('void main(){out_FragColor=vec4(1.0);}',{renderState:ref.RenderState.fromCache({depthTest:{enabled:true}})});command.shaderProgram=program;
    if(log)command=ref.DerivedCommand.createLogDepthCommand(command,context).command;
    if(hdr)command=ref.DerivedCommand.createHdrCommand(command,context).command;
    command.execute(context,pass);
    if(tile){globe.prepareColorTextures(context,false);globe.executeUpdateDepth(context,pass,globe.depthStencilTexture);}else globe.executeCopyDepth(context,pass);
    const probe=context.createViewportQuadCommand(new ref.ShaderSource({defines:['LOG_DEPTH_READ_ONLY'],sources:['in vec2 v_textureCoordinates;\nvoid main(){float d=czm_unpackDepth(texture(czm_globeDepthTexture,v_textureCoordinates));out_FragColor=vec4(d,d,d,1.0);}']}),{renderState:ref.RenderState.fromCache({depthMask:false})});
    probe.execute(context,pass);globe.prepareColorTextures(context,false);pass.framebuffer=undefined;globe.executeCopyColor(context,pass);
    const expected=Array.from(context.readPixels({width:8,height:8})),actual=JSON.parse(wasm.scene_oit(gl,JSON.stringify(config)));
    if(JSON.stringify(actual.pixels)!==JSON.stringify(expected)||gl.getError()!==0){const differences=expected.flatMap((v,i)=>v===actual.pixels[i]?[]:[[i,actual.pixels[i],v]]).slice(0,32);throw Error(JSON.stringify({config,differences,glError:gl.getError()}));}
    globe.destroy();context.destroy();passed.push(config);
  }
  return passed;
}
