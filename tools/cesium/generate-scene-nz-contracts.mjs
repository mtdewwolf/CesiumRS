// Reference cases from the project's pinned @cesium/engine 23.0.0.
import {writeFileSync} from 'node:fs';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import ShadowMapShader from '@cesium/engine/Source/Scene/ShadowMapShader.js';
import QuadtreeTileProvider from '@cesium/engine/Source/Scene/QuadtreeTileProvider.js';
import GeographicTilingScheme from '@cesium/engine/Source/Core/GeographicTilingScheme.js';
import Ellipsoid from '@cesium/engine/Source/Core/Ellipsoid.js';
import ParticleSystem from '@cesium/engine/Source/Scene/ParticleSystem.js';
import JulianDate from '@cesium/engine/Source/Core/JulianDate.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import ParticleBurst from '@cesium/engine/Source/Scene/ParticleBurst.js';
const output={version:'23.0.0',shadows:[],errors:[],particles:[]};
for(let mask=0;mask<32;mask++) {
 const [point,terrain,depth,opaque,varying]=[0,1,2,3,4].map(i=>!!(mask&(1<<i)));
 const source=(varying?'out vec3 v_positionEC;\n':'')+'void main() { out_FragColor = vec4(0.5); }';
 const s=new ShaderSource({sources:[source],defines:['EXISTING']});
 const serialize=s=>({sources:s.sources,defines:s.defines});
 output.shadows.push({point,terrain,depth,opaque,source,keyword:ShadowMapShader.getShadowCastShaderKeyword(point,terrain,depth,opaque),vertex:serialize(ShadowMapShader.createShadowCastVertexShader(s,point,terrain)),fragment:serialize(ShadowMapShader.createShadowCastFragmentShader(s,point,depth,opaque)),receive:serialize(ShadowMapShader.createShadowReceiveVertexShader(s,terrain,true))});
}
for(const radius of [1,6378137]) for(const tiles of [1,2,8]) {
 const scheme=new GeographicTilingScheme({ellipsoid:new Ellipsoid(radius,radius,radius),numberOfLevelZeroTilesX:tiles});
 output.errors.push({radius,tiles,error:QuadtreeTileProvider.computeDefaultLevelZeroMaximumGeometricError(scheme)});
}
// Isolate CPU behavior with the host billboard adapter; production update logic remains upstream.
const system=new ParticleSystem({emissionRate:2,particleLife:1,lifetime:2,loop:false,bursts:[new ParticleBurst({time:.5,minimum:2,maximum:2})],emitter:{emit(p){p.position=Cartesian3.ZERO.clone();p.velocity=Cartesian3.UNIT_Z.clone();}}});
system._updateParticlePool=false;
system._billboardCollection={add(){return {};},update(){},remove(){}};
const start=new JulianDate(2451545,0);let completed=0;system.complete.addEventListener(()=>completed++);
for(const t of [0,.25,.5,.75,1,1.5,2,2.5,3]) {
 system.update({time:JulianDate.addSeconds(start,t,new JulianDate()),frameNumber:1});
 output.particles.push({time:t,currentTime:system._currentTime,complete:system.isComplete,completed,particles:system._particles.map(p=>({age:p.age,z:p.position.z}))});
}
writeFileSync(new URL('../../tests/fixtures/scene/NzContracts.json',import.meta.url),JSON.stringify({version:output.version,cases:['shadows','errors','particles'].flatMap(kind=>output[kind].map(value=>({kind,...value})))},null,2)+'\n');
