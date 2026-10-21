import ComputeCommand from '@cesium/engine/Source/Renderer/ComputeCommand.js';
import FramebufferManager from '@cesium/engine/Source/Renderer/FramebufferManager.js';
import Sync from '@cesium/engine/Source/Renderer/Sync.js';
import ViewportQuadVS from '@cesium/engine/Source/Shaders/ViewportQuadVS.js';
import {suite} from './lib.mjs';

export default async function rendererExecution(){
 const s=suite('RendererExecution');
 s.add('viewportShader',[],()=>ViewportQuadVS);
 for(const options of [{},{persists:true,owner:'compute'},{persists:false,fragmentShaderSource:'void main(){}'}]){
  s.add('commandDefaults',[options],options=>{const c=new ComputeCommand(options);return [c.pass,c.persists,c.owner??null,c.fragmentShaderSource??null,c.vertexArray??null,c.shaderProgram??null,c.outputTexture??null,c.preExecute??null,c.postExecute??null,c.canceled??null,c.uniformMap??null];});
 }
 for(const options of [{},{numSamples:4},{color:false,depth:true},{color:false,depthStencil:true},{depth:true,supportsDepthTexture:true},{depthStencil:true,supportsDepthTexture:true},{createColorAttachments:false},{createDepthAttachments:false},{colorAttachmentsLength:3,pixelDatatype:5126,pixelFormat:6408}]){
  s.add('managerDefaults',[options],options=>{const m=new FramebufferManager(options);return [m.numSamples,m.isDirty(4,2),m._color,m._depth,m._depthStencil,m._supportsDepthTexture,m._createColorAttachments,m._createDepthAttachments,m._colorAttachmentsLength,m._pixelDatatype??null,m._pixelFormat??null];});
 }
 for(const statuses of [[true],[false,true],[false,false,true],[false,false,false]])for(const ttl of [0,1,2,null]){
  let polls=0,scheduled=0,success=true,message=null;
  const fake={getStatus:()=>statuses[polls++]?37145:37144};
  try{await Sync.prototype.waitForSignal.call(fake,next=>{scheduled++;next();},ttl??undefined);}catch(e){success=false;message=e.message;}
  s.add('syncPolling',[statuses,ttl],()=>[success,polls,scheduled,message]);
 }
 return [s];
}
