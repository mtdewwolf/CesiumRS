// Run pinned Cesium's public collection pick methods against loaded terrain data.
import {writeFileSync,readFileSync} from 'node:fs';
import ImageryLayer from './node_modules/@cesium/engine/Source/Scene/ImageryLayer.js';
import ImageryLayerCollection from './node_modules/@cesium/engine/Source/Scene/ImageryLayerCollection.js';
import GeographicTilingScheme from './node_modules/@cesium/engine/Source/Core/GeographicTilingScheme.js';
import Rectangle from './node_modules/@cesium/engine/Source/Core/Rectangle.js';
import Cartographic from './node_modules/@cesium/engine/Source/Core/Cartographic.js';
import Ellipsoid from './node_modules/@cesium/engine/Source/Core/Ellipsoid.js';

const defaultLayers=[{name:'bottom',features:[{name:'bottom-a'},{name:'bottom-b',position:[0.3,0.4,7]}]},{name:'top',features:[{name:'top-a'}]}];
const attachment=(layer,extra={})=>({layer,rectangle:[-1,-1,1,1],coordinates:[0,0,0],texture:[0,0,1,1],...extra});
const tile=(imagery,extra={})=>({rectangle:[-1,-1,1,1],imagery,...extra});
const scenarios=[];
function add(name,overrides={}) {
 scenarios.push({name,location:[0,0,123],layers:structuredClone(defaultLayers),tiles:[tile([attachment(0),attachment(1)])],...overrides});
}
add('top-to-bottom and position fallback');
add('ray misses globe',{hit:false});
add('no rendered terrain',{tiles:[]});
add('no containing terrain',{location:[2,0,123]});
add('empty imagery',{tiles:[tile([])]});
add('loading-only imagery',{tiles:[tile([attachment(0,{ready:false}),attachment(1,{ready:false})])]});
add('not-ready layer',{layers:[defaultLayers[0],{name:'top',ready:false}]});
add('no ready layers',{layers:[{name:'bottom',ready:false},{name:'top',ready:false}]});
add('no pickFeatures methods',{layers:[{name:'bottom',support:false},{name:'top',support:false}]});
add('synchronous unsupported pick',{layers:[{name:'bottom',response:'undefined'},{name:'top',response:'undefined'}]});
add('async undefined and empty',{layers:[{name:'bottom',response:'none'},{name:'top',features:[]}]});
add('mixed supported providers',{layers:[defaultLayers[0],{name:'top',support:false}]});
add('provider rejects',{layers:[defaultLayers[0],{name:'top',response:'error'}]});
add('lower provider rejects',{layers:[{name:'bottom',response:'error'},defaultLayers[1]]});
add('completion order differs from attachment order',{layers:[defaultLayers[0],{...defaultLayers[1],delay:3}]});
add('preserve supplied feature position',{layers:[{name:'bottom',features:[{name:'supplied',position:[-0.2,0.1,-50],description:'description',properties:{a:1},data:{raw:true}}]},defaultLayers[1]]});
add('duplicate layer attachments',{tiles:[tile([attachment(0),attachment(1),attachment(0,{coordinates:[2,3,4]})])]});
add('ancestor coordinates',{tiles:[tile([attachment(0,{rectangle:[-2,-1.5,2,1.5],coordinates:[1,2,3]})])]});
add('imagery misses hit',{tiles:[tile([attachment(0,{rectangle:[0.2,0.2,0.8,0.8]}),attachment(1)])]});
add('parent texture area misses hit',{tiles:[tile([attachment(0,{texture:[0.7,0.7,1,1]}),attachment(1)])]});
add('first terrain wins',{tiles:[tile([attachment(0)]),tile([attachment(1)])]});
add('first terrain empty still wins',{tiles:[tile([]),tile([attachment(1)])]});
add('later terrain contains hit',{tiles:[tile([attachment(0)],{rectangle:[1,0,2,1]}),tile([attachment(1)])]});
add('attached hidden layers remain pickable',{layers:defaultLayers.map(l=>({...l,show:false,alpha:0}))});
add('attached layers outside this collection',{layers:defaultLayers.map(l=>({...l,inCollection:false}))});
for(const [name,longitude] of [['inside',0],['within tolerance',0.0018],['outside tolerance',0.0021]]) {
 add(`parent texture edge ${name}`,{location:[longitude,0,123],tiles:[tile([attachment(0,{texture:[0,0,0.5,1]})])]});
}
for(const longitude of [3.1,-3.1]) {
 add(`antimeridian ${longitude}`,{location:[longitude,0.3,20],tiles:[tile([attachment(0,{rectangle:[3,-1,-3,1]})],{rectangle:[3,-1,-3,1]})]});
}
add('terrain shared edge uses first',{location:[0,0.2,9],tiles:[tile([attachment(0)],{rectangle:[-1,-1,0,1]}),tile([attachment(1)],{rectangle:[0,-1,1,1]})]});
add('nonzero hit longitude latitude and height',{location:[0.2,0.4,321]});
add('custom scene ellipsoid',{location:[0.2,0.4,2],ellipsoid:[2,3,4]});

const position=v=>v?new Cartographic(...v):undefined;
const encodeFeature=f=>({name:f.name??null,description:f.description??null,properties:f.properties??null,data:f.data??null,position:f.position?[f.position.longitude,f.position.latitude,f.position.height]:null,layer:f.imageryLayer.testName});
for(const scenario of scenarios) {
 const ellipsoid=scenario.ellipsoid?new Ellipsoid(...scenario.ellipsoid):Ellipsoid.WGS84;
 const calls=[]; const collection=new ImageryLayerCollection();
 const layers=scenario.layers.map(config=>{
  const provider={tilingScheme:new GeographicTilingScheme()};
  if(config.support!==false) provider.pickFeatures=(...args)=>{
   calls.push([config.name,...args]);
   if(config.response==='undefined')return undefined;
   return (async()=>{
    for(let i=0;i<(config.delay??0);i++)await Promise.resolve();
    if(config.response==='error')throw Error(`${config.name} rejected`);
    if(config.response==='none')return undefined;
    return (config.features??[]).map(f=>({...f,position:position(f.position)}));
   })();
  };
  const layer=new ImageryLayer(config.ready===false?undefined:provider,{show:config.show??true,alpha:config.alpha??1});
  layer.testName=config.name;
  if(config.inCollection!==false)collection.add(layer);
  return layer;
 });
 const terrain=scenario.tiles.map(tile=>({rectangle:new Rectangle(...tile.rectangle),data:{imagery:tile.imagery.map(a=>({textureCoordinateRectangle:{x:a.texture[0],y:a.texture[1],z:a.texture[2],w:a.texture[3]},readyImagery:a.ready===false?undefined:{imageryLayer:layers[a.layer],x:a.coordinates[0],y:a.coordinates[1],level:a.coordinates[2],rectangle:new Rectangle(...a.rectangle)}}))}}));
 const ray={test:'passed through'};let hits=0;
 const scene={ellipsoid,globe:{pick(r,s){if(r!==ray||s!==scene)throw Error('pick arguments');hits++;return scenario.hit===false?undefined:ellipsoid.cartographicToCartesian(position(scenario.location));},_surface:{_tilesToRender:terrain}}};
 const layerHits=collection.pickImageryLayers(ray,scene)?.map(l=>l.testName)??null;
 const future=collection.pickImageryLayerFeatures(ray,scene);let features=null,error=null;
 if(future)try{features=(await future).map(encodeFeature);}catch(e){error=e.message;}
 scenario.expected={layers:layerHits,hasFuture:future!==undefined,features,error,calls,hits};
}
const cesium=JSON.parse(readFileSync(new URL('./node_modules/@cesium/engine/package.json',import.meta.url))).version;
writeFileSync(new URL('../../tests/data/scene-imagery-picking.json',import.meta.url),JSON.stringify({cesium,scenarios},null,2)+'\n');
console.log(`${scenarios.length} pinned imagery-picking scenarios`);
