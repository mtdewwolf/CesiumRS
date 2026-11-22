import * as C from "@cesium/engine";
import { enc } from "./lib.mjs";

const time = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const single = (module, result) => ({ module, cases: [{ f: "lifecycle", a: [], r: enc(result) }] });

function custom() {
  const s = new C.CustomDataSource();
  const trace = [];
  s.changedEvent.addEventListener(source => trace.push(["change", source.name, !!source.clock]));
  s.loadingEvent.addEventListener((source, loading) => trace.push(["loading", loading, source.isLoading]));
  s.entities.collectionChanged.addEventListener((_, added) => trace.push(["entities", added.map(e => e.id), s.isLoading]));
  const initial = [s.name, !!s.clock, s.show, s.isLoading, s.entities.owner === s];
  s.name = "one";
  s.name = "one";
  s.clock = new C.DataSourceClock();
  s.clock = s.clock;
  s.clock = undefined;
  s.isLoading = true;
  s.isLoading = true;
  s.entities.add({id:"A"});
  s.entities.add({id:"B"});
  s.isLoading = false;
  s.isLoading = false;
  s.show = false;
  return [initial, trace, s.entities.show, s.update(time)];
}

async function collection() {
  const c = new C.DataSourceCollection();
  const [a,b,d] = ["A","B","A"].map(n => new C.CustomDataSource(n));
  const tag = s => [a,b,d].indexOf(s);
  const trace = [];
  c.dataSourceAdded.addEventListener((c,s) => trace.push(["add",tag(s),c.length]));
  c.dataSourceRemoved.addEventListener((c,s) => trace.push(["remove",tag(s),c.length]));
  c.dataSourceMoved.addEventListener((s,to,from) => trace.push(["move",tag(s),to,from]));
  for (const s of [a,b,d]) {
    s.destroy = () => trace.push(["destroy", tag(s)]);
    await c.add(s);
  }
  c.lower(a);
  c.raise(a);
  c.raiseToTop(b);
  c.lowerToBottom(d);
  c.lower(b);
  c.raise(a);
  const order = Array.from({length:c.length}, (_,i)=>tag(c.get(i)));
  const names = c.getByName("A").map(tag);
  const removed = c.remove(a,true);
  const absent = c.remove(a,true);
  c.destroy();
  return [trace,order,names,removed,absent,c.isDestroyed()];
}

async function display() {
  // Use the real display and primitive collections. Terrain data is already available on this host.
  const heights = C.ApproximateTerrainHeights;
  const saved = [heights._terrainHeights, heights._initPromise];
  heights._terrainHeights = {};
  heights._initPromise = Promise.resolve();
  try {
    const c = new C.DataSourceCollection();
    const trace = [];
    const a = new C.CustomDataSource("A");
    const b = new C.CustomDataSource("B");
    await c.add(a);
    const scene = { camera:{changed:new C.Event()}, postRender:new C.Event(),
      primitives:new C.PrimitiveCollection(),groundPrimitives:new C.PrimitiveCollection(),requestRender() {} };
    let ready = true;
    let pending = true;
    const d = new C.DataSourceDisplay({scene, dataSourceCollection:c, visualizersCallback(_,__,s) {
      const name = s.name ?? "default";
      trace.push(["create",name]);
      return [0,1,2].map(i => ({
        update() { trace.push(["update",name,i]); return ready; },
        destroy() { trace.push(["destroy",name,i]); },
        getBoundingSphere(e,r) {
          if (i===2) return pending ? C.BoundingSphereState.PENDING : C.BoundingSphereState.FAILED;
          C.BoundingSphere.clone(new C.BoundingSphere(new C.Cartesian3(i*6,0,0),1+i),r);
          return C.BoundingSphereState.DONE;
        },
      }));
    }});
    await c.add(b);
    c.raise(a);
    const e = b.entities.add({id:"B"});
    const def = d.defaultDataSource.entities.add({id:"default"});
    const outsider = new C.Entity({id:"B"});
    function bounds(e, partial) {
      const r = new C.BoundingSphere(new C.Cartesian3(2,3,4),5);
      return [d.getBoundingSphere(e,partial,r),enc(r)];
    }
    const frames = [d.ready,bounds(e,false),d.update(time),d.ready,bounds(e,false),bounds(e,true),bounds(outsider,true)];
    ready = false;
    pending = false;
    frames.push(d.update(time),d.ready,bounds(def,false));
    c.remove(b);
    frames.push(bounds(e,false));
    d.destroy();
    return [frames,trace,d.isDestroyed(),c.length];
  } finally { [heights._terrainHeights, heights._initPromise] = saved; }
}

const f = (id,geometry,properties) => ({type:"Feature",id,geometry,properties});
const g = (type,coordinates) => ({type,coordinates});
const fc = (...features) => ({type:"FeatureCollection",features});
const ring = [[10,20],[11,20],[11,22],[10,20]];
const hole = [[10.4,20.2],[10.6,20.3],[10.6,20.5],[10.4,20.2]];
const line = [[2,3],[4,5,6]];
const all = fc(
  f("pin",g("Point",[2,3]),{title:"Harbor","marker-color":"#08aec9","marker-size":"small","marker-symbol":"h",nested:{list:[1,null,false],title:"hidden"},count:0}),
  f("pin",g("MultiPoint",[[0,0],[1,2,3]]),{displayName:"display",NAME:"lookout","marker-symbol":"harbor","marker-size":"large",description:null}),
  f("line",g("LineString",line),{stroke:"rgba(120,60,30,0.5)","stroke-opacity":1,"stroke-width":3.5,description:"<em>Line</em>"}),
  f("lines",g("MultiLineString",[line,line]),{"stroke-opacity":0.3}),
  f("area",g("Polygon",[ring,hole]),{stroke:"#f80","stroke-opacity":0.45,"stroke-width":0,fill:"#123456","fill-opacity":0.8}),
  f("areas",g("MultiPolygon",[[ring],[ring.map(p=>[...p,42])],[]]),{fill:"red"}),
  f("collection",{type:"GeometryCollection",geometries:[g("Point",[10,20]),{type:"GeometryCollection",geometries:[g("LineString",line)]}]},{someTitle:"first",TITLE:"best"}),
  f("empty",null,{label:"a",name:"selected",description:""}),
  f("no props",g("Polygon",[]),null),
);
const topology = {type:"Topology", transform:{scale:[0.25,0.5],translate:[10,20]},
  arcs:[[[0,0],[4,0]],[[4,0],[0,4],[-4,-4]]],
  objects:{main:{type:"GeometryCollection",geometries:[
    {id:"line",type:"LineString",arcs:[0,1],properties:{title:"Joined"}},
    {id:"multi",type:"MultiLineString",arcs:[[-2,-1],[0]]},
    {id:"polygon",type:"Polygon",arcs:[[0,1],[0]]},
    {id:"polygons",type:"MultiPolygon",arcs:[[[0,1]],[[-2,-1]]]},
    {id:"points",type:"MultiPoint",coordinates:[[2,4,7],[1,2]]},
    {id:"nested",type:"GeometryCollection",geometries:[{type:"Point",coordinates:[8,6]}]},
  ]}, empty:{id:"void",type:null}}};
const opts = {sourceUri:"https://example.test/data/ports.geojson?q=1",clampToGround:true};
const op = (data,options={},method="load") => ({data,options,method});
const namedPoint = f("p",g("Point",[2,4,8]),{});
const geoCases = [
  ["all", [op(all,opts)]],
  ["overrides", [op(all,{markerColor:"green",markerSymbol:"q",markerSize:32,stroke:"cyan",strokeWidth:8,fill:"rgba(10,20,30,0.6)"})]],
  ["topology",[op(topology),op({...topology,transform:null},{},"process")]],
  ["reload",[op(f("old",null,{}),{sourceUri:"old.json"}),op(f("old",null,{}),{},"process"),op(namedPoint,opts),op({...namedPoint,crs:null})]],
  ["bare",[op({type:"GeometryCollection",geometries:[g("Point",[0,0]),g("Polygon",[ring])]})]],
  ...[["named",{type:"name",properties:{name:"fixture"}}],["href",{type:"link",properties:{href:"fixture",type:"fixture-type",offset:10}}],
    ["linkType",{type:"link",properties:{href:"other",type:"fixture-type",offset:20}}],["epsg",{type:"EPSG",properties:{code:4326}}],
    ["badName",{type:"name",properties:{name:"missing"}}],["badEpsg",{type:"EPSG",properties:{code:42}}],
    ["badLink",{type:"link",properties:{href:"missing"}}],["missingProps",{type:"link"}],["badType",{type:"other",properties:{}}]]
    .map(([label,crs])=>[label,[op(namedPoint),op({...namedPoint,crs},opts)]]),
  ["badRoot",[op(namedPoint),op({type:"NotGeometry"},opts)]],
  ["badFeature",[op(fc(f("kept",null,{}),{type:"Feature",id:"bad"}))]],
  ["badGeometry",[op(f("bad",{type:"Other"},{}))]],
];
function snapshot(e,id) {
  const prop = p => enc(p?.getValue(time));
  const fields = (obj,keys) => obj ? Object.fromEntries(keys.map(k=>[k,prop(obj[k])])) : null;
  return {id:id(e),name:enc(e.name),description:prop(e.description),properties:prop(e.properties),position:prop(e.position),
    billboard:fields(e.billboard,["image","verticalOrigin","heightReference"]),
    polyline:fields(e.polyline,["positions","material","arcType","width","clampToGround"]),
    polygon:fields(e.polygon,["hierarchy","material","arcType","outline","outlineColor","outlineWidth","perPositionHeight","height"])};
}
async function geo(ops) {
  const s = new C.GeoJsonDataSource("initial");
  const pin = (kind,symbol,color,size) => ({kind,symbol,color:enc(color),size});
  s._pinBuilder = {
    fromColor:(c,z)=>pin("color",null,c,z), fromText:(t,c,z)=>pin("text",t,c,z), fromMakiIconId:(t,c,z)=>pin("maki",t,c,z),
  };
  const auto = new Map();
  const id = e => {
    if (!/^[a-f0-9]{8}-[a-f0-9-]{27}$/i.test(e.id)) return e.id;
    if (!auto.has(e.id)) auto.set(e.id,"@"+auto.size);
    return auto.get(e.id);
  };
  const trace = [];
  s.changedEvent.addEventListener(s=>trace.push(["name",s.name]));
  s.errorEvent.addEventListener((s,e)=>trace.push(["error",e.message]));
  s.loadingEvent.addEventListener((s,v)=>trace.push(["loading",v]));
  s.entities.collectionChanged.addEventListener((_,a,r,c)=>trace.push(["entities",a.map(id),r.map(id),c.map(id)]));
  const results = [];
  for (const {data,options,method} of ops) {
    const style = {...options};
    for (const k of ["stroke","fill","markerColor"]) if (style[k]) style[k]=C.Color.fromCssColorString(style[k]);
    let error;
    try { await s[method](data,style); } catch (e) { error=e.message; }
    results.push({name:s.name,entities:s.entities.values.map(e=>snapshot(e,id)),trace:trace.splice(0),error:enc(error)});
  }
  return results;
}

export default async function dataSourcesCollections() {
  const geoSuite = {module:"GeoJsonDataSource",cases:[]};
  const crs = C.GeoJsonDataSource;
  const project = offset => ([x,y,z=0])=>new C.Cartesian3(x+offset,y,z);
  crs.crsNames.fixture = project(1);
  crs.crsLinkHrefs.fixture = p=>Promise.resolve(project(p.offset+1));
  crs.crsLinkTypes["fixture-type"] = p=>Promise.resolve(project(p.offset+2));
  try {
    for (const [f,ops] of geoCases) geoSuite.cases.push({f,a:[ops],r:await geo(ops)});
  } finally {
    delete crs.crsNames.fixture; delete crs.crsLinkHrefs.fixture; delete crs.crsLinkTypes["fixture-type"];
  }
  const states = C.BoundingSphereState;
  return [single("BoundingSphereState",[states.DONE,states.PENDING,states.FAILED]),single("CustomDataSource",custom()),
    single("DataSourceCollection",await collection()),single("DataSourceDisplay",await display()),geoSuite];
}
