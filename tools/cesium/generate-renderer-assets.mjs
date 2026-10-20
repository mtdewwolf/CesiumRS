// Retain pinned GLSL text and automatic-uniform declarations as data for Rust.
// Dependency resolution and uniform evaluation are implemented in Rust.
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import AutomaticUniforms from '@cesium/engine/Source/Renderer/AutomaticUniforms.js';
import {mkdirSync,writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../../assets/renderer/',import.meta.url));mkdirSync(root,{recursive:true});
writeFileSync(root+'glsl-builtins.json',JSON.stringify(ShaderSource._czmBuiltinsAndUniforms)+'\n');
writeFileSync(root+'automatic-uniforms.json',JSON.stringify(Object.fromEntries(Object.entries(AutomaticUniforms).map(([k,v])=>[k,{datatype:v._datatype,size:v._size}])) )+'\n');
console.log(`${Object.keys(ShaderSource._czmBuiltinsAndUniforms).length} GLSL dependencies; ${Object.keys(AutomaticUniforms).length} automatic uniforms`);
