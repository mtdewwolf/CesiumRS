import U from '@cesium/engine/Source/Scene/Model/ModelUtility.js';
import {Matrix4,Cartesian3,Quaternion} from '@cesium/engine';
import {suite,enc,makeRng} from './lib.mjs';
export function sceneModelUtility(){const s=suite('ModelUtility');
 for(const name of ['', 'gl_position','gl__x','1234','_COLOR_0','foo  !!bar','été名字','a_b___c','gl_123','a\n\tb','GL_foo'])s.add('sanitize',[name],U.sanitizeGlslIdentifier);
 for(let up=0;up<3;up++)for(let forward=0;forward<3;forward++)s.add('axis',[up,forward],(u,f)=>U.getAxisCorrectionMatrix(u,f,new Matrix4()));
 const rng=makeRng(8231);for(let i=0;i<40;i++){const scale=[rng()*4-2,rng()*4-2,rng()*4-2],m=Matrix4.fromScale(new Cartesian3(...scale));for(let p=0;p<7;p++)s.add('cull',[enc(m),p],(_,p)=>U.getCullFace(m,p));}
 for(const name of ['_TEMPERATURE','_color_12','gl_hello','_1test'])for(const type of ['SCALAR','VEC2','VEC3','VEC4','MAT4'])for(const quantized of [false,true]){const a={name,type,...(quantized?{quantization:{type:'VEC2'}}:{})};s.add('info',[a],a=>{const {attribute,...info}=U.getAttributeInfo(a);return info;});}
 for(const semantic of ['POSITION','NORMAL','TANGENT','COLOR','TEXCOORD','JOINTS','WEIGHTS','_FEATURE_ID'])for(const setIndex of [undefined,0,2]){const a={name:'test',semantic,setIndex,type:'VEC3'};s.add('info',[a],a=>{const {attribute,...info}=U.getAttributeInfo(a);return info;});}
 for(const a of [{},{translation:[1,2,3]},{translation:[1,2,3],rotation:[0,0,Math.SQRT1_2,Math.SQRT1_2],scale:[2,3,4]},{matrix:Array.from(Matrix4.IDENTITY),translation:[3,4,5]}])s.add('transform',[a],a=>U.getNodeTransform({...a,translation:a.translation?new Cartesian3(...a.translation):undefined,rotation:a.rotation?new Quaternion(...a.rotation):undefined,scale:a.scale?new Cartesian3(...a.scale):undefined,matrix:a.matrix?Matrix4.fromArray(a.matrix):undefined}));
 for(const required of [[],Object.keys(U.supportedExtensions),['KHR_mesh_quantization','NO_SUCH_EXTENSION']])s.add('extensions',[required],v=>{U.checkSupportedExtensions(v);return true;});
 return s;}
