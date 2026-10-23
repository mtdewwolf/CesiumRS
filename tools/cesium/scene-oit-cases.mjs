export async function run(wasm,ref,canvas,attributes){
  const passed=[];
  for(const mrt of [false,true])for(const log of [false,true])for(const hdr of [false,true])for(const reverse of [false,true]){
    const config={mrt,log,hdr,reverse};window.lastSceneCase={oit:true,...config};
    const gl=canvas().getContext('webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes});
    context.uniformState._frameState={mode:3};context.uniformState._mode=3;context.uniformState._backgroundColor=ref.Color.BLACK;
    context.uniformState.updateFrustum(new ref.PerspectiveFrustum({fov:Math.PI/3,aspectRatio:1,near:700,far:1313}));
    // Engine 23.0.0's fallback omits this output declaration while its
    // redirected color variable suppresses ShaderSource's automatic one.
    // Match the native compile adapter's explicit declaration; shader-transform
    // fixtures separately compare the unmodified source transformation.
    if(!mrt){const create=context.shaderCache.createDerivedShaderProgram;context.shaderCache.createDerivedShaderProgram=function(program,key,options){if(key==='translucentMultipass'||key==='alphaMultipass')options.fragmentShaderSource.sources.unshift('layout (location = 0) out vec4 out_FragColor;\n');return create.call(this,program,key,options);};}
    const viewport=new ref.BoundingRectangle(0,0,8,8),pass=new ref.PassState(context);pass.viewport=viewport;
    const opaque=new ref.FramebufferManager({depthStencil:true,supportsDepthTexture:true});opaque.update(context,8,8,1,hdr?(context.halfFloatingPointTexture?36193:5126):5121);
    const oit=new ref.OIT({colorBufferFloat:context.colorBufferFloat,depthTexture:context.depthTexture,floatBlend:context.floatBlend,drawBuffers:mrt&&context.drawBuffers});
    if(!oit.isSupported())throw Error('Reference OIT unavailable');
    oit.update(context,pass,opaque,hdr,1);oit.clear(context,pass,ref.Color.BLACK);
    pass.framebuffer=opaque.framebuffer;new ref.ClearCommand({depth:1,stencil:0}).execute(context,pass);
    const program=ref.ShaderProgram.fromCache({context,vertexShaderSource:'in vec4 position;\nuniform float distance;\nuniform float scale;\nvoid main(){gl_Position=czm_projection*vec4(position.xy*scale*distance*0.5,-distance,1.0);}',fragmentShaderSource:'uniform vec4 color;\nvoid main(){out_FragColor=color;}',attributeLocations:{position:0}}),commands=[];
    for(const i of reverse?[0,3,2,1]:[0,1,2,3]){
      let command=context.createViewportQuadCommand('void main(){out_FragColor=vec4(1.0);}',{renderState:ref.RenderState.fromCache({depthTest:{enabled:true},depthMask:i===0,blending:i===0?{enabled:false}:{enabled:true,functionSourceRgb:770,functionSourceAlpha:1,functionDestinationRgb:771,functionDestinationAlpha:771}}),uniformMap:{distance:()=>[1100,1000,950,900][i],scale:()=>i===0?.65:1,color:()=>[new ref.Color(.2,.3,.4,1),new ref.Color(1,0,0,.4),new ref.Color(0,1,0,.5),new ref.Color(0,0,1,.2)][i]}});
      command.shaderProgram=program;
      let derived=command;
      if(log){const d=ref.DerivedCommand.createLogDepthCommand(derived,context);derived.derivedCommands.logDepth=d;derived=d.command;}
      if(hdr){const d=ref.DerivedCommand.createHdrCommand(derived,context);derived.derivedCommands.hdr=d;derived=d.command;}
      if(i===0)derived.execute(context,pass);else{derived.derivedCommands.oit=oit.createDerivedCommands(derived,context);commands.push(command);}
    }
    oit.executeCommands({context,frameState:{useLogDepth:log,shadowState:{lightShadowsEnabled:false}},_hdr:hdr},(command,scene,state)=>command.execute(scene.context,state),pass,commands);
    pass.framebuffer=undefined;oit.execute(context,pass);
    const expected=Array.from(context.readPixels({width:8,height:8})),actual=JSON.parse(wasm.scene_oit(gl,JSON.stringify(config)));
    if(!actual.supported||JSON.stringify(actual.pixels)!==JSON.stringify(expected)||gl.getError()!==0){const differences=expected.flatMap((v,i)=>v===actual.pixels[i]?[]:[[i,actual.pixels[i],v]]).slice(0,32);throw Error(JSON.stringify({config,differences,glError:gl.getError(),supported:actual.supported}));}
    oit.destroy();opaque.destroy();context.destroy();passed.push({oit:true,...config});
  }return passed;
}
