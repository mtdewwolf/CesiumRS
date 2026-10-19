import * as C from '@cesium/engine';
import {enc,makeRng} from './lib.mjs';
const suite=module=>({module,cases:[],async add(f,args,fn){let r;try{r=enc(await fn(...args));}catch(e){r={throws:e.message,kind:e.name};}this.cases.push({f,a:args.map(enc),r});}});
export default async function coreServices(){
 const out=[],oldJson=C.Resource.prototype.fetchJson,oldJsonp=C.Resource.prototype.fetchJsonp;
 try {
  const examples={
   BingMapsGeocoderService:[{resourceSets:[]},{resourceSets:[{resources:[{name:'Bing point',bbox:[10,20,30,40]},{name:'Dateline',bbox:[-5,179,5,-179]}]}]}],
   PeliasGeocoderService:[{features:[]},{features:[{properties:{label:'point'},geometry:{coordinates:[-75,40]}},{properties:{label:'area'},bbox:[-80,30,-60,50]}],attributions:[{html:'Source',collapsible:false},{html:'Other',collapsible:true}]}],
   OpenCageGeocoderService:[{results:[]},{results:[{formatted:'point',geometry:{lat:45,lng:-90}},{formatted:'area',bounds:{southwest:{lat:40,lng:-100},northeast:{lat:50,lng:-80}}}]}],
   GoogleGeocoderService:[{status:'ZERO_RESULTS'},{status:'OK',results:[{formatted_address:'Google result',geometry:{viewport:{southwest:{lng:-100,lat:40},northeast:{lng:-80,lat:50}}}}]},{status:'REQUEST_DENIED',error_message:'invalid key'},{status:null},{status:12,error_message:{}}]
  };
  for(const [name,responses] of Object.entries(examples)){
   const s=suite(name);out.push(s);
   for(const response of responses) for(const kind of [0,1]) await s.add('geocode',[response,kind],async(response,kind)=>{
    let request;C.Resource.prototype.fetchJson=function(){request=[this.url,null];return Promise.resolve(response);};C.Resource.prototype.fetchJsonp=function(parameter){request=[this.url,parameter];return Promise.resolve(response);};
    const service=name==='BingMapsGeocoderService'?new C[name]({key:'test key',culture:'fr-FR'}):name==='GoogleGeocoderService'?new C[name]({key:'test key'}):name==='PeliasGeocoderService'?new C[name]('https://geo.test/v1?token=a'):new C[name]('https://geo.test/v1?token=a','test key',{language:'fr'});
    const result=await service.geocode('New York & Paris',kind);
    return [request,service.credit?[service.credit.html,service.credit.showOnScreen]:null,result.map(r=>[r.displayName,r.destination,r.attributions??null,r.attribution??null])];
   });
  }
 } finally {C.Resource.prototype.fetchJson=oldJson;C.Resource.prototype.fetchJsonp=oldJsonp;}
 const tile=suite('TileProviderError');out.push(tile);
 for(const ops of [['error','error','success','error','error'],['error','success','success','error']]) await tile.add('retry',[ops],ops=>{
  let previous;const seen=[],event=new C.Event();event.addEventListener(e=>{seen.push([e.message,e.timesRetried,e.retry,e.x,e.y,e.level]);e.retry=e.timesRetried<2;});
  const states=ops.map((op,i)=>{if(op==='success')C.TileProviderError.reportSuccess(previous);else previous=C.TileProviderError.reportError(previous,{},event,'error-'+i,i,i+1,3,'details');return [previous.timesRetried,previous.retry];});return [seen,states];
 });
 const video=suite('VideoSynchronizer');out.push(video);const rng=makeRng(504);
 for(const loop of [false,true]) for(const readyState of [1,2,4]) await video.add('ticks',[loop,readyState,Array.from({length:30},(_,i)=>[i%3!==0,Math.floor(rng()*50)-10,i%5===0?-1:2,i%4===0])],(loop,readyState,ticks)=>{
  const calls=[],el={readyState,paused:true,currentTime:0,duration:20,loop,addEventListener(_,fn){this.seeked=fn;},removeEventListener(){},play(){this.paused=false;calls.push('play');},pause(){this.paused=true;calls.push('pause');},set playbackRate(v){calls.push(['rate',v]);if(v<0)throw Error('unsupported');}};
  const epoch=C.JulianDate.fromIso8601('2020-01-01T00:00:00Z'),sync=new C.VideoSynchronizer({element:el,epoch,tolerance:0.5});
  return ticks.map(([shouldAnimate,time,multiplier,seeked])=>{if(seeked)el.seeked();sync._onTick({shouldAnimate,currentTime:C.JulianDate.addSeconds(epoch,time,new C.JulianDate()),multiplier});const c=calls.splice(0);return [el.currentTime,el.paused,c];});
 });
 for(const name of ['sampleTerrain','sampleTerrainMostDetailed']){
  const s=suite(name);out.push(s);
  for(const mode of ['static','metadata','throttled']) for(const fail of [false,true]) for(const reject of [false,true]) await s.add('sample',[fail,reject,mode],async(fail,reject,mode)=>{
   const calls=[],scheme=new C.GeographicTilingScheme(),availability=new C.TileAvailability(scheme,2);availability.addAvailableTileRange(0,0,0,1,0);if(mode!=='metadata')availability.addAvailableTileRange(2,0,0,7,3);let throttled=false;
   const provider={tilingScheme:scheme,availability,requestTileGeometry(x,y,level){calls.push([x,y,level]);if(mode==='throttled'&&!throttled){throttled=true;return undefined;}if(fail&&x%2===0)return Promise.reject(Error('tile failed'));return Promise.resolve(new C.HeightmapTerrainData({buffer:Float32Array.from([10,20,30,40].map(v=>v+level*100)),width:2,height:2}));},loadTileDataAvailability(){if(mode==='metadata'){availability.addAvailableTileRange(2,0,0,7,3);return Promise.resolve();}return undefined;}};
   const positions=[[-160,20],[-150,25],[10,20],[170,-20]].map(([x,y])=>C.Cartographic.fromDegrees(x,y,99));
   let rejected=false;try{if(name==='sampleTerrain')await C.sampleTerrain(provider,2,positions,reject);else await C.sampleTerrainMostDetailed(provider,positions,reject);}catch{rejected=true;}
   // Drain concurrent tile work after a rejection before observing mutations.
   await Promise.resolve();return [positions.map(p=>p.height??'NaN'),calls,rejected];
  });
 }
 return out;
}
