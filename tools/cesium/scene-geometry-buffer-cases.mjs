// Real WebGL uploads, indexed draws, readbacks and cache lease destruction.
export async function run(wasm,ref,canvas,attributes){
 const passed=[];
 for(const webgl1 of [false,true])for(const componentType of [5121,5123,5125])for(const asynchronous of [false,true])for(const retain of [false,true]){
  ref.ResourceCache.clearForSpecs();
  const positions=new Float32Array([-1,-1,3,-1,-1,3]);
  const Index=componentType===5121?Uint8Array:componentType===5123?Uint16Array:Uint32Array;
  const indices=new Index([0,1,2]),raw=new Uint8Array(25+indices.byteLength);raw.set(new Uint8Array(positions.buffer));raw.set(new Uint8Array(indices.buffer),25);
  const gltf={buffers:[{byteLength:raw.length}],bufferViews:[{buffer:0,byteOffset:0,byteLength:24},{buffer:0,byteOffset:24,byteLength:1+indices.byteLength}],accessors:[{bufferView:1,byteOffset:1,componentType,type:'SCALAR',count:3}]};
  const config={webgl1,asynchronous,retain,gltf,raw:Array.from(raw)};window.lastSceneCase={geometry:true,webgl1,componentType,asynchronous,retain};
  const gl=canvas().getContext(webgl1?'webgl':'webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes,requestWebgl1:webgl1});
  // Pinned Context queries MAX_3D_TEXTURE_SIZE unconditionally on WebGL1.
  const initializationError=context._gl.getError();if(initializationError!==0&&!(webgl1&&initializationError===1280))throw Error('Unexpected reference context initialization error '+initializationError);
  const resource=new ref.Resource('https://geometry.test/triangle.gltf');
  const seed=ref.ResourceCache.getEmbeddedBufferLoader({parentResource:resource,bufferId:0,typedArray:raw});
  let allowance=0;const frame={context,jobScheduler:{execute(job){if(allowance===0)return false;allowance--;job.execute();return true;}}};
  const options={gltf,gltfResource:resource,baseResource:resource,frameState:frame,loadBuffer:true,loadTypedArray:retain,asynchronous};
  const vertex=ref.ResourceCache.getVertexBufferLoader({...options,bufferViewId:0}),duplicate=ref.ResourceCache.getVertexBufferLoader({...options,bufferViewId:0}),index=ref.ResourceCache.getIndexBufferLoader({...options,accessorId:0});
  await vertex.load();await index.load();const deferred=[vertex.process(frame),index.process(frame)];allowance=2;vertex.process(frame);index.process(frame);
  const vb=vertex.buffer,ib=index.buffer;
  const read=buffer=>{const out=new Uint8Array(buffer.sizeInBytes);buffer.getBufferData(out);return Array.from(out);};
  const readback=webgl1?null:[read(vb),read(ib)];
  const va=new ref.VertexArray({context,attributes:[{index:0,vertexBuffer:vb,componentsPerAttribute:2}],indexBuffer:ib});
  const program=ref.ShaderProgram.fromCache({context,vertexShaderSource:'in vec2 position;void main(){gl_Position=vec4(position,0.,1.);}',fragmentShaderSource:'void main(){out_FragColor=vec4(0.25,0.5,0.75,1.);}',attributeLocations:{position:0}});
  const pass=new ref.PassState(context);new ref.ClearCommand({color:ref.Color.BLACK,depth:1,stencil:0}).execute(context,pass);
  new ref.DrawCommand({vertexArray:va,shaderProgram:program,renderState:ref.RenderState.fromCache({})}).execute(context,pass);
  const pixels=Array.from(context.readPixels({width:8,height:8})),retained=[!!vertex.typedArray,!!index.typedArray],geometry=ref.ResourceCache.statistics.geometryByteLength;
  ref.ResourceCache.unload(vertex);const aliveWithLease=!vb.isDestroyed();ref.ResourceCache.unload(duplicate);ref.ResourceCache.unload(index);ref.ResourceCache.unload(seed);
  const expected={deferred,shared:vertex===duplicate,readback,pixels,retained,geometry,aliveWithLease,destroyed:[vb.isDestroyed(),ib.isDestroyed()],remaining:Object.keys(ref.ResourceCache.cacheEntries).length,finalGeometry:ref.ResourceCache.statistics.geometryByteLength};
  const actual=JSON.parse(await wasm.scene_geometry_buffers(gl,JSON.stringify(config)));
  for(const key of Object.keys(expected))if(JSON.stringify(expected[key])!==JSON.stringify(actual[key]))throw Error(JSON.stringify({geometry:true,config:window.lastSceneCase,key,expected:expected[key],actual:actual[key]}));
  const actualError=gl.getError(),expectedError=context._gl.getError();if(actualError!==0||expectedError!==0)throw Error(JSON.stringify({geometry:true,actualError,expectedError}));
  passed.push(window.lastSceneCase);va.destroy();program.destroy();context.destroy();
 }
 return passed;
}
