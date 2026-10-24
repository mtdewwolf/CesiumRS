// Direct property scenarios from pinned @cesium/engine; no Rust-derived outputs.
import * as C from "@cesium/engine";
import { suite, enc, makeRng } from "./lib.mjs";
const epoch = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const date = t => C.JulianDate.addSeconds(epoch, t, new C.JulianDate());
const seconds = t => t === undefined ? null : C.JulianDate.secondsDifference(t, epoch);
const types = { number: Number, Cartesian2: C.Cartesian2, Cartesian3: C.Cartesian3, Cartesian4: C.Cartesian4, Color: C.Color, Quaternion: C.Quaternion, Matrix2: C.Matrix2, Matrix3: C.Matrix3, Matrix4: C.Matrix4 };
function decode(type, v) {
  if (v === "undefined") return undefined;
  if (v === null) return null;
  if (type === "number") return typeof v === "string" ? Number(v) : v;
  if (type.startsWith("Matrix")) return types[type].fromColumnMajorArray(v);
  return new types[type](...v);
}
function interval(a, data) {
  return new C.TimeInterval({ start: date(a[0]), stop: date(a[1]), isStartIncluded: a[2], isStopIncluded: a[3], data });
}
const queries = [-20, -1, 0, 0.1, 1, 2, 4.5, 5, 6, 9.9, 10, 11, 15, 20, 21, 40];
export default function dataSourcesProperties() {
  const all = [];
  const constant = suite("ConstantProperty");
  for (const [type, values] of [
    ["number", ["undefined", null, 1, 1, -0, "NaN", "NaN", "Infinity", -4, null, "undefined"]],
    ["Cartesian2", [[1,2],[1,2],[3,4],null,"undefined"]],
    ["Cartesian3", [[1,2,3],[1,2,3],[4,5,6],null,"undefined"]],
    ["Cartesian4", [[1,2,3,4],[1,2,3,4],[4,5,6,7]]],
    ["Color", [[0.1,0.2,0.3,0.4],[0.1,0.2,0.3,0.4],[1,1,0,1]]],
    ["Quaternion", [[0,0,0,1],[0,0,0,1],[0,1,0,0]]],
    ["Matrix2", [[1,2,3,4],[1,2,3,4],[0,0,0,0]]],
    ["Matrix3", [[1,2,3,4,5,6,7,8,9],[1,2,3,4,5,6,7,8,9]]],
    ["Matrix4", [Array.from({length:16},(_,i)=>i),Array.from({length:16},(_,i)=>i+1)]],
  ]) constant.add("sequence", [type, values], (type, values) => {
    const p = new C.ConstantProperty(); let changes = 0;
    p.definitionChanged.addEventListener(() => changes++);
    return values.map(v => {
      p.setValue(decode(type,v));
      const other = new C.ConstantProperty(decode(type,v));
      return [p.isConstant, enc(p.getValue(date(5))), String(p), changes, p.equals(other), p.equals(p), enc(p.valueOf())];
    });
  });
  constant.add("reference", [], () => {
    const object = { a: 1 }; const p = new C.ConstantProperty(object); let changes=0;
    p.definitionChanged.addEventListener(()=>changes++);
    p.setValue(object); const same=p.equals(new C.ConstantProperty(object));
    object.a=2; const value=enc(p.getValue()); p.setValue({a:2});
    return [same, value, changes, p.equals(new C.ConstantProperty({a:2})), String(p)];
  });
  all.push(constant);

  const callback = suite("CallbackProperty");
  for (const flag of [false,true]) for (const times of [[0,1,10],[-10,0.25,500]]) callback.add("evaluate", [flag,times], (flag,times)=>{
    let calls=0,changes=0; const cb=(time,result)=>{calls++; const t=seconds(time); const old=result.x; result.x=t; result.y=t*2; result.z=old; return result;};
    const p=new C.CallbackProperty(cb,flag); p.definitionChanged.addEventListener(()=>changes++);
    const result=new C.Cartesian3(7,8,9); const values=times.map(t=>enc(p.getValue(date(t),result)));
    const eq=p.equals(new C.CallbackProperty(cb,flag)); p.setCallback(cb,flag); p.setCallback(cb,!flag); p.setCallback(cb,!flag);
    const different=p.equals(new C.CallbackProperty(()=>1,!flag));
    return [values, enc(result), calls, changes, p.isConstant, eq, different];
  });
  all.push(callback);

  const helpers=suite("Property");
  for (const v of ["absent","undefined",null,0,2,"NaN"]) helpers.add("helpers",[v], v=>{
    const p=v==="absent"?undefined:new C.ConstantProperty(decode("number",v));
    return [C.Property.isConstant(p), enc(C.Property.getValueOrUndefined(p,date(0))), enc(C.Property.getValueOrDefault(p,date(0),9)), C.Property.equals(p,p), C.Property.equals(p,undefined), C.Property.arrayEquals([p],[p]), C.Property.arrayEquals(undefined,[])];
  });
  helpers.add("clonedDefault",[],()=>{
    const p=new C.CallbackProperty((t,r)=>{r.x=77;return undefined;},false);
    const result=new C.Cartesian3(1,2,3), fallback=new C.Cartesian3(4,5,6);
    const value=C.Property.getValueOrClonedDefault(p,date(0),fallback,result); value.x=99;
    return [enc(value),enc(result),enc(fallback),value===result,value===fallback];
  });
  helpers.add("eventEdits",[],()=>{
    const p=new C.ConstantProperty(0),e=p.definitionChanged,trace=[];let edited=false,removeB;
    const c=()=>trace.push("c");
    e.addEventListener(()=>{trace.push("a");if(!edited){edited=true;removeB();e.addEventListener(c);}});
    removeB=e.addEventListener(()=>trace.push("b"));p.setValue(1);p.setValue(2);
    const fn=()=>{};const remove1=e.addEventListener(fn),remove2=e.addEventListener(fn);const before=e.numberOfListeners;remove1();remove2();
    return [trace,before,e.numberOfListeners];
  });
  helpers.add("eventReentry",[],()=>{
    const p=new C.ConstantProperty(0),trace=[];let nested=false;
    const c=()=>trace.push("c");p.definitionChanged.addEventListener(()=>{trace.push("a");if(!nested){nested=true;p.definitionChanged.addEventListener(c);p.setValue(2);}});
    p.definitionChanged.addEventListener(()=>trace.push("b"));p.setValue(1);
    return [trace,p.definitionChanged.numberOfListeners];
  });
  all.push(helpers);

  const array=suite("PropertyArray");
  for (const values of [[],[1,2,3],[null,1,"undefined",2],["undefined"],[1,1,null]]) for (const variable of [false,true]) array.add("sequence",[values,variable],(values,variable)=>{
    const children=values.map(v=>variable?new C.CallbackProperty(()=>decode("number",v),false):new C.ConstantProperty(decode("number",v)));
    const p=new C.PropertyArray(children.length?[...children,children[0]]:[]);let changes=0;p.definitionChanged.addEventListener(()=>changes++);
    const other=new C.PropertyArray(children.length?[...children,children[0]]:[]);const equal=p.equals(other);other.setValue(undefined);
    const initial=[p.isConstant,enc(p.getValue(date(0))),children.map(c=>c.definitionChanged.numberOfListeners),equal];
    if(children.length) {if(variable)children[0].setCallback(()=>5,true);else children[0].setValue(5);}
    const changed=[p.isConstant,enc(p.getValue(date(1))),changes];
    p.setValue(undefined); const cleared=[p.isConstant,enc(p.getValue(date(1))),changes,children.map(c=>c.definitionChanged.numberOfListeners)];
    p.setValue([]);return [initial,changed,cleared,enc(p.getValue(date(1))),changes];
  });
  all.push(array);

  for (const composite of [false,true]) {
    const s=suite(composite?"CompositeProperty":"TimeIntervalCollectionProperty");
    for (const type of ["number","Cartesian3"]) for (const variant of [0,1,2,3]) {
      const values=type==="number"?[1,1,7]:[[1,2,3],[1,2,3],[7,8,9]];
      const operations=[ ["clear"], ["add",0,10,true,true,0], ["add",10,20,variant%2===0,true,1], ["add",4,6,true,variant<2,2], ["remove",5,15,variant%2===0,variant<2], ["set",2,type==="number"?8:[8,9,10]], ["add",0,0,false,false,0], ["clear"], ["clear"] ];
      s.add("operations",[type,values,operations,queries],(type,values,operations,queries)=>{
        const data=values.map(v=>composite?new C.ConstantProperty(decode(type,v)):decode(type,v));
        const p=composite?new C.CompositeProperty():new C.TimeIntervalCollectionProperty();let changes=0;p.definitionChanged.addEventListener(()=>changes++);
        return operations.map(op=>{
          let removed=null;
          if(op[0]==="add")p.intervals.addInterval(interval(op.slice(1),data[op[5]]));
          if(op[0]==="remove")removed=p.intervals.removeInterval(interval(op.slice(1)));
          if(op[0]==="clear")p.intervals.removeAll();
          if(op[0]==="set"&&composite)data[op[1]].setValue(decode(type,op[2]));
          return [changes,p.isConstant,p.intervals.length,queries.map(t=>enc(p.getValue(date(t)))),removed,composite?data.map(p=>p.definitionChanged.numberOfListeners):null];
        });
      });
    }
    for (const type of ["number","Cartesian3"]) for (const equal of [false,true]) s.add("equals",[type,equal],(type,equal)=>{
      const a=composite?new C.CompositeProperty():new C.TimeIntervalCollectionProperty(),b=composite?new C.CompositeProperty():new C.TimeIntervalCollectionProperty();
      const av=decode(type,type==="number"?1:[1,2,3]),bv=decode(type,type==="number"?(equal?1:2):(equal?[1,2,3]:[4,5,6]));
      a.intervals.addInterval(interval([0,10,true,false],composite?new C.ConstantProperty(av):av));
      b.intervals.addInterval(interval([0,10,true,false],composite?new C.ConstantProperty(bv):bv));
      return a.equals(b);
    });
    all.push(s);
  }

  const sampled=suite("SampledProperty"),rng=makeRng(0xdada1234);
  for (const type of ["number","Cartesian2","Cartesian3","Color","Quaternion"]) for (const algorithm of ["LinearApproximation","LagrangePolynomialApproximation","HermitePolynomialApproximation"]) for (const degree of [1,2,3,5]) for (const derivatives of (type==="number"||type==="Cartesian3"?[0,1,2]:[0])) {
    const times=[10,0,20,5,15,10];
    const size=type==="number"?1:type==="Cartesian2"?2:type==="Cartesian3"?3:4;
    const values=times.map((t,i)=>type==="number"?t*t+3*t+i:Array.from({length:size},(_,j)=>Math.sin(t*0.1+j)+i*0.1));
    if(type==="Quaternion")for(const v of values) {const length=Math.hypot(...v);for(let i=0;i<4;i++)v[i]/=length;}
    const d=times.map(t=>Array.from({length:derivatives},(_,k)=>Array.from({length:size},(_,j)=>k===0?2*t+3+j:2+j)));
    sampled.add("samples",[type,algorithm,degree,derivatives,times,values,d,queries],(type,algorithm,degree,derivatives,times,values,d,queries)=>{
      const p=new C.SampledProperty(types[type],derivatives?Array(derivatives).fill(types[type]):undefined);let changes=0;p.definitionChanged.addEventListener(()=>changes++);
      p.setInterpolationOptions({interpolationAlgorithm:C[algorithm],interpolationDegree:degree});
      p.addSamples(times.map(date),values.map(v=>decode(type,v)),derivatives?d.map(row=>row.map(v=>decode(type,type==="number"?v[0]:v))):undefined);
      const initial=[queries.map(t=>enc(p.getValue(date(t)))),changes,p.isConstant,[-7,-1,0,4,5].map(i=>seconds(p.getSample(i)))];
      p.forwardExtrapolationType=C.ExtrapolationType.HOLD;p.backwardExtrapolationType=C.ExtrapolationType.HOLD;
      const hold=queries.map(t=>enc(p.getValue(date(t))));
      p.forwardExtrapolationType=C.ExtrapolationType.EXTRAPOLATE;p.backwardExtrapolationType=C.ExtrapolationType.EXTRAPOLATE;p.forwardExtrapolationDuration=3;p.backwardExtrapolationDuration=3;
      const extrap=queries.map(t=>enc(p.getValue(date(t))));
      const removed=[p.removeSample(date(7)),p.removeSample(date(10))];p.removeSamples(interval([4,15,false,true]));
      const final=[queries.map(t=>enc(p.getValue(date(t)))),changes,p.isConstant,[-1,0,1,2].map(i=>seconds(p.getSample(i)))];
      return [initial,hold,extrap,removed,final];
    });
  }
  for(let i=0;i<40;i++) {
    const times=Array.from({length:i%11},()=>Math.floor(rng()*40));const values=times.map(()=>rng()*100-50);
    sampled.add("packed",[i%3,times,values,queries],(kind,times,values,queries)=>{
      const p=new C.SampledProperty(Number);let changes=0;p.definitionChanged.addEventListener(()=>changes++);
      p.addSamplesPackedArray(times.flatMap((t,i)=>[kind===0?t:kind===1?date(t):C.JulianDate.toIso8601(date(t)),values[i]]),kind===0?epoch:undefined);
      const other=new C.SampledProperty(Number);other.addSamplesPackedArray(times.flatMap((t,i)=>[t,values[i]]),epoch);other.forwardExtrapolationType=C.ExtrapolationType.HOLD;
      const eq=p.equals(other);p.removeSamples(interval([100,200,true,true]));
      return [queries.map(t=>enc(p.getValue(date(t)))),changes,p.isConstant,eq];
    });
  }
  sampled.add("emptyDerivatives",[],()=>{
    const p=new C.SampledProperty(Number,[]),other=new C.SampledProperty(Number);let changes=0;p.definitionChanged.addEventListener(()=>changes++);
    p.addSample(date(0),1);p.addSamples([date(10)],[3]);other.addSamples([date(0),date(10)],[1,3]);
    return [enc(p.getValue(date(5))),p.equals(other),changes,p.derivativeTypes.length];
  });
  all.push(sampled);
  return all;
}
