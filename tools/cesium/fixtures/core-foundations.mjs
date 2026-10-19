// Reference cases for implementations whose validation was deferred.
import * as C from '@cesium/engine';
import TexturePacker from '@cesium/engine/Source/Core/TexturePacker.js';
import RectangleCollisionChecker from '@cesium/engine/Source/Core/RectangleCollisionChecker.js';
import decodeVectorPolylinePositions from '@cesium/engine/Source/Core/decodeVectorPolylinePositions.js';
import decodeGoogleEarthEnterpriseData from '@cesium/engine/Source/Core/decodeGoogleEarthEnterpriseData.js';
import {suite, makeRng} from './lib.mjs';
export default function coreFoundations() {
  const out=[], add=name=>{const s=suite(name);out.push(s);return s;};
  const p=add('DefaultProxy');
  for(const proxy of ['/proxy','/proxy?','/proxy?url=','https://proxy.test/?a=1&'])
    for(const url of ['tiles/a b?x=é&y=1#hash','https://EXAMPLE.test:443/a','data:text/plain,a+b'])
      p.add('getUrl',[proxy,url],(a,b)=>new C.DefaultProxy(a).getURL(b));
  const t=add('TrustedServers');
  for(const host of ['example.test','EXAMPLE.test','xn--bcher-kva.test','[::1]']) t.add('registry',[host],host=>{
    C.TrustedServers.clear();C.TrustedServers.add(host,443);
    const urls=['https://example.test/a','https://user:pw@EXAMPLE.test:443/a','http://example.test/a','//example.test:443/a','https://bücher.test/a','https://[::1]:443/a','/relative'];
    const before=urls.map(u=>C.TrustedServers.contains(u));C.TrustedServers.remove(host,443);
    return [before,urls.map(u=>C.TrustedServers.contains(u))];
  });
  const ct=add('CompressedTextureBuffer');
  for(const args of [[33776,5121,4,8,[0,1,255]],[6408,null,1,1,[]]]) ct.add('clone',args,(f,d,w,h,b)=>{
    const v=new C.CompressedTextureBuffer(f,d??undefined,w,h,Uint8Array.from(b)),c=C.CompressedTextureBuffer.clone(v);
    return [c.internalFormat,c.pixelDatatype,c.width,c.height,c.bufferView,c.bufferView===v.arrayBufferView];
  });
  const credit=add('Credit');
  for(const html of ['hello','<img src="ion-credit.png">','<a href="https://example.test">Example</a>'])
    for(const screen of [false,true]) credit.add('identity',[html,screen],(html,screen)=>{
      const a=new C.Credit(html,screen),b=C.Credit.clone(a),c=new C.Credit(html,!screen);
      return [a.html,a.showOnScreen,a.id===b.id,a.equals(b),a.equals(c),a.isIon(),C.Credit.getIonCredit({html,collapsible:false}).showOnScreen];
    });
  const err=add('RequestErrorEvent');
  for(const status of [null,404,500]) for(const headers of [null,'Content-Type: text/plain\r\nX-Test: a:b\r\nX-Test: last\r\n'])
    err.add('error',[status,headers],(status,headers)=>{const e=new C.RequestErrorEvent(status??undefined,'body',headers??undefined);return [e.statusCode,e.response,e.responseHeaders,e.toString()];});
  const tile=add('GoogleEarthEnterpriseTileInformation');
  for(let bits=0;bits<256;bits++) tile.add('flags',[bits],bits=>{
    const a=new C.GoogleEarthEnterpriseTileInformation(bits,2,3,4,5,6),parent=new C.GoogleEarthEnterpriseTileInformation(128,0,0,0,0,0);
    a.setParent(parent);const b=C.GoogleEarthEnterpriseTileInformation.clone(a);
    return [b.hasSubtree(),b.hasImagery(),b.hasTerrain(),b.hasChildren(),[0,1,2,3].map(i=>b.hasChild(i)),b.getChildBitmask(),b.ancestorHasTerrain,b.cnodeVersion,b.imageryVersion,b.terrainVersion,b.imageryProvider,b.terrainProvider];
  });
  const rng=makeRng(3591),pack=add('TexturePacker');
  for(const padding of [0,1,2,0.5]) for(const size of [8,32,128]) {
    const images=Array.from({length:50},()=>[1+Math.floor(rng()*20),1+Math.floor(rng()*20)]);
    pack.add('pack',[size,padding,images],(size,padding,images)=>{const p=new TexturePacker({width:size,height:size,borderPadding:padding});return images.map(([width,height],i)=>p.pack(i,{width,height})?.rectangle??null);});
  }
  const rect=add('RectangleCollisionChecker');
  for(let round=0;round<16;round++) {
    const boxes=Array.from({length:150},()=>{const x=Math.floor(rng()*20),y=Math.floor(rng()*20);return [x,y,x+rng()*3,y+rng()*3];});
    const ops=[...boxes.map((r,i)=>['insert',String(i%41),r]),...Array.from({length:250},()=>[rng()<0.45?'remove':'collides',String(Math.floor(rng()*41)),boxes[Math.floor(rng()*boxes.length)]])];
    rect.add('operations',[ops],ops=>{const c=new RectangleCollisionChecker();return ops.map(([op,id,r])=>{const b=new C.Rectangle(...r);if(op==='collides')return c.collides(b);c[op](id,b);return null;});});
  }
  const gee=add('decodeGoogleEarthEnterpriseData');
  for(const kl of [24,32,48,1024]) for(const length of [4,5,7,8,9,23,24,25,64,127]) {
    const key=Array.from({length:kl},(_,i)=>(i*7+3)%256),bytes=Array.from({length},(_,i)=>(i*11+7)%256);
    gee.add('decode',[key,bytes],(key,bytes)=>{const b=Uint8Array.from(bytes);decodeGoogleEarthEnterpriseData(Uint8Array.from(key).buffer,b.buffer);return b;});
  }
  for(const b of [[173,222,104,116],[116,104,222,173]]) gee.add('decode',[[1,2,3,4],b],(k,b)=>{const a=Uint8Array.from(b);decodeGoogleEarthEnterpriseData(Uint8Array.from(k).buffer,a.buffer);return a;});
  const poly=add('decodeVectorPolylinePositions');
  for(const e of ['WGS84','UNIT_SPHERE']) for(let i=0;i<20;i++) {
    const b=Array.from({length:30},()=>Math.floor(rng()*65536));
    poly.add('decode',[b,[-2,-1,2,1],-100,8000,e],(b,r,min,max,e)=>{const data=Uint16Array.from(b);return [decodeVectorPolylinePositions(data,new C.Rectangle(...r),min,max,C.Ellipsoid[e]),data];});
  }
  const resource=add('Resource');
  for(const url of ['https://EXAMPLE.test/a/{z}/b.png?a=1&a=2&x=hello+world#hash','../tiles?raw','data:text/plain;charset=utf-8,a%20b','//server.test/path/{x}?b=2'])
    for(const preserve of [false,true]) resource.add('derive',[url,preserve],(url,preserve)=>{
      const a=new C.Resource({url,queryParameters:{a:'base',z:['3','4']},templateValues:{z:'a/b',x:'é'},headers:{Accept:'x'}});
      a.appendQueryParameters({a:['appended'],new:'v'});
      const b=a.getDerivedResource({url:'../other/{x}?a=derived&u=1',preserveQueryParameters:preserve,queryParameters:{q:'a b'},headers:{X:'y'}});
      const encode=r=>[r.url,r.getUrlComponent(false,false),r.queryParameters,r.templateValues,r.headers,r.extension,r.getBaseUri(true),r.isDataUri,r.isBlobUri];
      return [encode(a),encode(b),encode(a.clone())];
    });
  const cart=add('CartographicGeocoderService');
  for(const q of ['0,0','-75.5 40 123','45N 100W','100W 45N','90S 180E 0','0x10 0b10','0o20 0x1','1e2 -2e1','Infinity 0','12foo 4','45Q 100W','','   ','1','1 2 3 4','45.5north 75.2west','\uFEFF1,2','1_0 2','1 2 NaN','+0 -0'])
    cart.add('geocode',[q],q=>{let result;const original=Promise.resolve;Promise.resolve=v=>{result=v;return v;};try{new C.CartographicGeocoderService().geocode(q);return result.map(r=>[r.displayName,r.destination]);}finally{Promise.resolve=original;}});
  return out;
}
