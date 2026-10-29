import S from './node_modules/@cesium/engine/Source/Scene/ShadowMapShader.js';
import Shader from './node_modules/@cesium/engine/Source/Renderer/ShaderSource.js';
import fs from 'node:fs';
function source(kind,normal=true,pos=true,offset=true,terrain=false,polygon=true,debug=false) {
 const bias={normalShading:true,normalShadingSmooth:0.1,normalOffset:offset};
 const map={_isPointLight:kind===0,_isSpotLight:kind===1,_numberOfCascades:kind===2?4:1,_usesDepthTexture:true,_polygonOffsetSupported:polygon,debugCascadeColors:debug,softShadows:true,_pointBias:bias,_terrainBias:bias,_primitiveBias:bias};
 const shader=new Shader({sources:[`${pos?'in vec3 v_positionEC;':''}${normal?'in vec3 v_normalEC;':''}void main() { out_FragColor = vec4(1.0); }`]});
 return S.createShadowReceiveFragmentShader(shader,map,true,terrain,normal);
}
const base=source(3).sources.at(-1);
const pieces={};
pieces.uniforms=base.slice(base.indexOf('uniform mat4'),base.indexOf('vec4 getPositionEC'));
pieces.main=base.slice(base.indexOf('void main()'),base.indexOf('    float nDotL'));
for(let k=0;k<4;k++) { const s=source(k).sources.at(-1); pieces[['point','spot','cascade','directional'][k]]=s.slice(s.indexOf(k===0?'    vec3 directionEC':k===1?'    vec3 directionEC':k===2?'    float maxDepth':'    float nDotL'),s.indexOf('    out_FragColor.rgb')); }
fs.writeFileSync('src/scene/shadow_receive_glsl.rs', '// GLSL blocks ported from @cesium/engine 23.0.0 ShadowMapShader.\n'+Object.entries(pieces).map(([k,v])=>`const ${k.toUpperCase()}: &str = ${JSON.stringify(v)};`).join('\n')+'\n');
const cases=[];
for(let i=0;i<1024;i++) {const f=Array.from({length:10},(_,b)=>!!(i&(1<<b)));const bias={normalShading:f[7],normalShadingSmooth:f[8]?0.1:0,normalOffset:f[9]}; const map={_usesDepthTexture:f[0],_polygonOffsetSupported:f[1],_isPointLight:f[2],_isSpotLight:f[3],_numberOfCascades:f[4]?4:1,debugCascadeColors:f[5],softShadows:f[6],_pointBias:bias,_terrainBias:bias,_primitiveBias:bias};for(const variant of [0,1,2,3]) {const shader=new Shader({sources:[`${variant&1?'in vec3 v_positionEC;':''}${variant&2?'in vec3 v_normalEC;':''}void main() { out_FragColor = vec4(1.0); }`]}); const terrain=variant===3;const r=S.createShadowReceiveFragmentShader(shader,map,true,terrain,!!(variant&2));let hash=0xcbf29ce484222325n; for(const byte of new TextEncoder().encode(JSON.stringify(r.sources))) { hash=BigInt.asUintN(64,(hash^BigInt(byte))*0x100000001b3n); } cases.push({f,variant,defines:r.defines,hash:hash.toString(16)});}}
fs.writeFileSync('tests/data/model-effects/shadow-receive.json',JSON.stringify(cases));

const {default: ShadowVolume} = await import('./node_modules/@cesium/engine/Source/Scene/ShadowVolumeAppearance.js');
const {default: PerInstance} = await import('./node_modules/@cesium/engine/Source/Scene/PerInstanceColorAppearance.js');
const {default: Geographic} = await import('./node_modules/@cesium/engine/Source/Core/GeographicProjection.js');
const volume=[];
for(let mask=0;mask<16;mask++) {
 const cull=!!(mask&1),planar=!!(mask&2),flat=!!(mask&4),cv=!!(mask&8);
 const appearance=new ShadowVolume(cull,planar,new PerInstance({flat}));
 const color=appearance.createFragmentShader(cv),pick=appearance.createPickFragmentShader(cv);
 const vs=appearance.createVertexShader(['EXTERNAL'],'void main() {}',cv,new Geographic());
 const pvs=appearance.createPickVertexShader(['EXTERNAL'],'void main() {}',cv,new Geographic());
 volume.push({mask,color:color.defines,pick:pick.defines,vertex:vs.defines,pickVertex:pvs.defines});
}
fs.writeFileSync('tests/data/model-effects/shadow-volume.json',JSON.stringify(volume,null,2));
