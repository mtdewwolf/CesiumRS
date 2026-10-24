import * as C from "@cesium/engine";
import createPropertyDescriptor from "@cesium/engine/Source/DataSources/createPropertyDescriptor.js";
import createRawPropertyDescriptor from "@cesium/engine/Source/DataSources/createRawPropertyDescriptor.js";
import { suite, enc } from "./lib.mjs";
const epoch = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const date = (t) => C.JulianDate.addSeconds(epoch, t, new C.JulianDate());
const span = (a, b, data) => new C.TimeInterval({ start: date(a), stop: date(b), data });
class FixtureComponent {
  constructor(value = {}) {
    this._definitionChanged = new C.Event();
    this.merge(value);
  }
  get definitionChanged() {
    return this._definitionChanged;
  }
  getType() {
    return "Fixture";
  }
  getValue(t, r = {}) {
    r.a = C.Property.getValueOrUndefined(this.a, t);
    r.b = C.Property.getValueOrUndefined(this.b, t);
    return r;
  }
  merge(v) {
    this.a = this.a ?? v.a;
    this.b = this.b ?? v.b;
  }
  clone(r = new FixtureComponent()) {
    r.a = this.a;
    r.b = this.b;
    return r;
  }
  equals(v) {
    return v instanceof FixtureComponent && C.Property.equals(this.a, v.a) && C.Property.equals(this.b, v.b);
  }
}
Object.defineProperties(FixtureComponent.prototype, { a: createPropertyDescriptor("a"), b: createPropertyDescriptor("b") });
function input(spec, pool) {
  if (spec === null) return void 0;
  const [kind, ...v] = spec;
  if (kind === "raw") return v[0];
  if (kind === "vec") return new C.Cartesian3(...v[0]);
  if (kind === "quat") return new C.Quaternion(...v[0]);
  if (kind === "property") {
    if (!pool.has(v[0])) pool.set(v[0], new C.ConstantProperty(v[1]));
    return pool.get(v[0]);
  }
  if (kind === "callback") {
    if (!pool.has(v[0])) pool.set(v[0], new C.CallbackProperty((t) => v[1] + C.JulianDate.secondsDifference(t, epoch), false));
    return pool.get(v[0]);
  }
  if (kind === "object") {
    if (!pool.has(v[0])) pool.set(v[0], v[1]);
    return pool.get(v[0]);
  }
  if (kind === "component") {
    if (!pool.has(v[0])) pool.set(v[0], new FixtureComponent(v[1]));
    return pool.get(v[0]);
  }
  throw Error(kind);
}
function describe(v) {
  if (v === void 0) return ["undefined"];
  if (v instanceof C.Entity) return ["entity", v.id];
  if (v instanceof C.TimeIntervalCollection) return ["availability", C.JulianDate.toIso8601(v.start), C.JulianDate.toIso8601(v.stop)];
  if (v && typeof v.getValue === "function") return ["property", enc(v.getValue(date(0)))];
  return ["raw", enc(v)];
}
function changeTrace(trace) {
  return (owner, name, value, old) => trace.push([name, describe(value), describe(old), value === old]);
}
function bagCapture(p, r, changes, pool) {
  const value = p.getValue(date(2), r);
  return [p.propertyNames.slice(), p.isConstant, enc(value), p.propertyNames.map((n) => [n, describe(p[n])]), changes, [...pool.values()].filter((v) => v?.definitionChanged).map((v) => v.definitionChanged.numberOfListeners)];
}
function entityCapture(e) {
  return [e.id, e.name ?? null, e.show, e.isShowing, e.parent?.id ?? null, e.trackingReferenceFrame, e.propertyNames.slice(), e.propertyNames.map((n) => [n, describe(e[n])]), e.availability ? [C.JulianDate.toIso8601(e.availability.start), C.JulianDate.toIso8601(e.availability.stop)] : null];
}
function dataSourcesEntities() {
  if (!Object.getOwnPropertyDescriptor(C.Entity.prototype, "fixtureData")) C.Entity.registerEntityType("fixtureData", FixtureComponent);
  const out = [];
  for (const raw of [false, true]) {
    const s = suite(raw ? "createRawPropertyDescriptor" : "createPropertyDescriptor");
    for (const configurable of [false, true]) for (const customFactory of raw ? [false] : [false, true]) {
      const ops = customFactory ? [["set", ["raw", 1]], ["set", ["raw", 1]], ["set", ["raw", null]], ["set", null]] : [["set", ["raw", 1]], ["set", ["raw", 1]], ["set", ["raw", null]], ["set", ["property", "p", 3]], ["set", ["property", "p", 3]], ["mutate", "p", 4], ["set", ["object", "o", { x: 1 }]], ["set", ["object", "o", { x: 1 }]], ["set", null], ["set", null]];
      s.add("slot", [raw, configurable, customFactory, ops], (raw2, configurable2, customFactory2, ops2) => {
        const owner = { _definitionChanged: new C.Event() }, trace = [], pool = /* @__PURE__ */ new Map();
        owner._definitionChanged.addEventListener(changeTrace(trace));
        const descriptor = raw2 ? createRawPropertyDescriptor("field", configurable2) : createPropertyDescriptor("field", configurable2, customFactory2 ? ((v) => new C.ConstantProperty(v === null ? 99 : v * 2)) : void 0);
        Object.defineProperty(owner, "field", descriptor);
        return ops2.map((op) => {
          if (op[0] === "set") owner.field = input(op[1], pool);
          else pool.get(op[1]).setValue(op[2]);
          return [describe(owner.field), trace.splice(0), descriptor.configurable, [...pool.values()].filter((v) => v?.definitionChanged).map((v) => v.definitionChanged.numberOfListeners)];
        });
      });
    }
    out.push(s);
  }
  const bag = suite("PropertyBag");
  for (const variant of [0, 1, 2]) {
    const ops = [["add", "a", variant === 0 ? ["raw", 1] : variant === 1 ? ["property", "p", 1] : ["callback", "p", 1]], ["add", "b", null], ["set", "b", ["raw", null]], ["set", "a", ["property", "q", 5]], ["mutate", "q", 6], ["merge", [["a", ["raw", 99]], ["c", ["raw", 3]], ["d", null]]], ["set", "b", null], ["remove", "c"], ["add", "e", ["raw", null]], ["merge", [["e", ["raw", null]]]]];
    bag.add("operations", [ops], (ops2) => {
      const p = new C.PropertyBag(), pool = /* @__PURE__ */ new Map(), result = { kept: 42 };
      let changes = 0;
      p.definitionChanged.addEventListener(() => changes++);
      return ops2.map((op) => {
        if (op[0] === "add") p.addProperty(op[1], input(op[2], pool));
        if (op[0] === "set") p[op[1]] = input(op[2], pool);
        if (op[0] === "mutate") pool.get(op[1]).setValue(op[2]);
        if (op[0] === "remove") p.removeProperty(op[1]);
        if (op[0] === "merge") {
          const source = {};
          for (const [n, v] of op[1]) source[n] = input(v, pool);
          p.merge(source);
        }
        return bagCapture(p, result, changes, pool);
      });
    });
  }
  for (const order of [false, true]) for (const equal of [false, true]) bag.add("equals", [order, equal], (order2, equal2) => {
    const a = new C.PropertyBag({ a: 1, b: 2 }), b = new C.PropertyBag(order2 ? { b: 2, a: equal2 ? 1 : 3 } : { a: equal2 ? 1 : 3, b: 2 });
    return [a.equals(b), a.equals(a), a.equals(new C.ConstantProperty(1))];
  });
  bag.add("mergeable", [], () => {
    const source = new FixtureComponent({ a: 1 }), target = new FixtureComponent({ b: 2 });
    const a = new C.PropertyBag({ x: source }), b = new C.PropertyBag({ x: target });
    a.merge(b);
    const clone = new C.PropertyBag();
    clone.merge(a);
    return [enc(a.getValue(date(0))), enc(clone.getValue(date(0))), clone.x === a.x, source.b === target.b];
  });
  out.push(bag);
  bag.add("numericKeys", [], () => {
    const p = new C.PropertyBag({ "10": 10, "2": 2, "01": 1, "4294967295": 4, "0": 0 }), r = p.getValue(date(0));
    const q = new C.PropertyBag();
    q.addProperty("10", 10);
    q.addProperty("2", 2);
    return [p.propertyNames, Object.keys(r), enc(r), q.propertyNames, Object.keys(q.getValue(date(0)))];
  });
  const entity = suite("Entity");
  const entityOps = [["add", "custom"], ["set", "custom", ["raw", 3]], ["set", "custom", ["raw", 3]], ["set", "description", ["raw", "first"]], ["set", "description", ["raw", "first"]], ["set", "position", ["vec", [6378137, 0, 0]]], ["set", "orientation", ["quat", [0, 0, 0, 1]]], ["set", "properties", ["raw", { x: 1 }]], ["set", "fixtureData", ["raw", { a: 1 }]], ["name", "renamed"], ["show", false], ["show", false], ["show", true], ["tracking", 2], ["remove", "custom"]];
  entity.add("operations", [entityOps], (ops) => {
    const e = new C.Entity({ id: "item", name: "initial" }), trace = [], pool = /* @__PURE__ */ new Map();
    e.definitionChanged.addEventListener(changeTrace(trace));
    return ops.map((op) => {
      if (op[0] === "add") e.addProperty(op[1]);
      if (op[0] === "remove") e.removeProperty(op[1]);
      if (op[0] === "set") e[op[1]] = input(op[2], pool);
      if (op[0] === "name") e.name = op[1];
      if (op[0] === "show") e.show = op[1];
      if (op[0] === "tracking") e.trackingReferenceFrame = op[1];
      return [entityCapture(e), trace.splice(0)];
    });
  });
  for (const flags of [[true, true, true], [false, true, true], [true, false, true], [true, true, false]]) entity.add("hierarchy", [flags], (flags2) => {
    const [p, c, g] = ["p", "c", "g"].map((id, i) => new C.Entity({ id, show: flags2[i] }));
    const trace = [];
    for (const e of [p, c, g]) e.definitionChanged.addEventListener((owner, name, v, o) => trace.push([owner.id, name, describe(v), describe(o)]));
    c.parent = p;
    g.parent = c;
    const result = [];
    for (const fn of [() => {
    }, () => p.show = false, () => p.show = true, () => c.show = false, () => g.parent = p, () => g.parent = void 0]) {
      fn();
      result.push([[p, c, g].map((e) => [e.id, e.isShowing, e.parent?.id ?? null]), trace.splice(0)]);
    }
    return result;
  });
  for (const orientation of [null, [0, 0, 0, 1], [0, 0, Math.SQRT1_2, Math.SQRT1_2]]) for (const position of [null, [6378137, 0, 0], [0, 0, 0], [100, 200, 300]]) for (const reference2 of [0, 1, 2, 3, 4, 5, 6]) entity.add("matrix", [position, orientation, reference2, 10], (position2, orientation2, reference3, offset) => {
    const e = new C.Entity({ id: "m", position: position2 ? new C.Cartesian3(...position2) : void 0, orientation: orientation2 ? new C.Quaternion(...orientation2) : void 0 });
    const result = new C.Matrix4(...Array(16).fill(7));
    const base = enc(e.computeModelMatrix(date(0), result)), after = enc(result);
    const height = enc(e.computeModelMatrixForHeightReference(date(0), new C.ConstantProperty(reference3), offset, C.Ellipsoid.WGS84, result));
    return [base, after, height, enc(result)];
  });
  entity.add("buffers", [], () => {
    let positions = 0, orientations = 0;
    const p = new C.CallbackPositionProperty((t, r) => {
      positions++;
      if (r === void 0) return void 0;
      r.x = 6378137;
      r.y = 0;
      r.z = 0;
      return r;
    }, false);
    const q = new C.CallbackProperty((t, r) => {
      orientations++;
      if (r === void 0) return void 0;
      r.x = 0;
      r.y = 0;
      r.z = 0;
      r.w = 1;
      return r;
    }, false);
    const e = new C.Entity({ id: "s", position: p, orientation: q });
    return [enc(e.computeModelMatrix(date(0))), enc(e.computeModelMatrixForHeightReference(date(0), new C.ConstantProperty(0), 0, C.Ellipsoid.WGS84)), positions, orientations];
  });
  entity.add("propertiesOrder", [], () => {
    const e = new C.Entity({ id: "n", properties: { "10": 10, "2": 2, "01": 1 } });
    return [e.properties.propertyNames, Object.keys(e.properties.getValue(date(0)))];
  });
  entity.add("availability", [], () => {
    const e = new C.Entity({ id: "a" }), trace = [];
    e.definitionChanged.addEventListener(changeTrace(trace));
    const a = new C.TimeIntervalCollection([span(0, 10)]);
    e.availability = a;
    e.availability = a;
    const first = [[-1, 0, 5, 10, 11].map((t) => e.isAvailable(date(t))), trace.splice(0)];
    a.addInterval(span(20, 30));
    const second = [[-1, 0, 10, 15, 20, 30, 31].map((t) => e.isAvailable(date(t))), trace.splice(0)];
    e.availability = void 0;
    return [first, second, e.isAvailable(date(100)), trace];
  });
  entity.add("merge", [], () => {
    const a = new C.Entity({ id: "a", name: "kept", description: "A", fixtureData: { a: 1 } }), b = new C.Entity({ id: "b", name: "ignored", description: "B", position: new C.Cartesian3(1, 2, 3), fixtureData: { b: 2 } });
    a.merge(b);
    return [entityCapture(a), a.fixtureData === b.fixtureData, a.fixtureData.a === b.fixtureData.a, a.fixtureData.b === b.fixtureData.b];
  });
  out.push(entity);
  const collection = suite("EntityCollection");
  for (const suspended of [false, true]) {
    const ops = [["add", "a"], ["add", "b"], ["name", "a", "one"], ["name", "a", "two"], ["remove", "b"], ["add", "b"], ["show", false], ["show", true], ["remove", "a"], ["add", "a"], ["all"]];
    collection.add("operations", [suspended, ops], (suspended2, ops2) => {
      const c = new C.EntityCollection(), entities = /* @__PURE__ */ new Map(), trace = [];
      c.collectionChanged.addEventListener((c2, a, r, h) => trace.push([a.map((e) => e.id), r.map((e) => e.id), h.map((e) => e.id)]));
      if (suspended2) c.suspendEvents();
      const out2 = ops2.map((op) => {
        if (op[0] === "add") {
          if (!entities.has(op[1])) entities.set(op[1], new C.Entity({ id: op[1] }));
          c.add(entities.get(op[1]));
        }
        if (op[0] === "name") entities.get(op[1]).name = op[2];
        if (op[0] === "remove") c.removeById(op[1]);
        if (op[0] === "show") c.show = op[1];
        if (op[0] === "all") c.removeAll();
        return [c.values.map((e) => [e.id, e.name ?? null, e.isShowing]), trace.splice(0)];
      });
      if (suspended2) c.resumeEvents();
      return [out2, trace];
    });
  }
  collection.add("identity", [], () => {
    const c = new C.EntityCollection(), a = new C.Entity({ id: "x" }), b = new C.Entity({ id: "x" });
    c.add(a);
    const before = [c.contains(a), c.contains(b), c.getOrCreateEntity("x") === a];
    const removed = c.remove(b);
    const n = c.getOrCreateEntity("x");
    return [before, removed, c.contains(a), n === a, c.values.map((e) => e.id)];
  });
  collection.add("reentrant", [], () => {
    const c = new C.EntityCollection(), trace = [];
    let once = false;
    c.collectionChanged.addEventListener((c2, a, r, h) => {
      trace.push([a.map((e) => e.id), r.map((e) => e.id), h.map((e) => e.id)]);
      if (!once) {
        once = true;
        c2.add(new C.Entity({ id: "b" }));
        c2.getById("a").name = "changed";
        c2.removeById("b");
      }
    });
    c.add(new C.Entity({ id: "a" }));
    return [trace, c.values.map((e) => [e.id, e.name ?? null])];
  });
  collection.add("availability", [], () => {
    const c = new C.EntityCollection();
    const out2 = [];
    const capture = () => {
      const a = c.computeAvailability();
      return [C.JulianDate.toIso8601(a.start), C.JulianDate.toIso8601(a.stop)];
    };
    out2.push(capture());
    c.add(new C.Entity({ id: "a", availability: new C.TimeIntervalCollection([span(1, 3)]) }));
    out2.push(capture());
    c.add(new C.Entity({ id: "b", availability: new C.TimeIntervalCollection([span(-2, 2), span(10, 20)]) }));
    out2.push(capture());
    c.add(new C.Entity({ id: "c" }));
    out2.push(capture());
    c.removeById("a");
    out2.push(capture());
    return out2;
  });
  out.push(collection);
  const reference = suite("ReferenceProperty");
  for (const path of [["description"], ["properties", "x"], ["fixtureData", "a"]]) reference.add("lifecycle", [path], (path2) => {
    const c = new C.EntityCollection(), r = new C.ReferenceProperty(c, "target", path2), trace = [];
    r.definitionChanged.addEventListener(() => trace.push("changed"));
    const out2 = [];
    const capture = () => [enc(r.getValue(date(0))), r.isConstant, r.referenceFrame ?? null, trace.splice(0)];
    out2.push(capture());
    const e = new C.Entity({ id: "target", description: "one", properties: { x: 1 }, fixtureData: { a: 2 } });
    c.add(e);
    out2.push(capture());
    if (path2[0] === "description") e.description = "two";
    else if (path2[0] === "properties") e.properties.x = 3;
    else e.fixtureData.a = 4;
    out2.push(capture());
    c.remove(e);
    out2.push(capture());
    c.add(new C.Entity({ id: "target", description: "next", properties: { x: 5 }, fixtureData: { a: 6 } }));
    out2.push(capture());
    return out2;
  });
  for (const text of ["abc#description", "a\\#b#properties.a\\.b", "x#fixtureData.a", "id#name\\\\part", "abc", "a#", "a#.x", "a#x."]) reference.add("parse", [text], (text2) => {
    const c = new C.EntityCollection();
    try {
      const r = C.ReferenceProperty.fromString(c, text2);
      return [r.targetId, r.targetPropertyNames];
    } catch (e) {
      return { error: e.message };
    }
  });
  reference.add("type", [], () => {
    const c = new C.EntityCollection(), r = new C.ReferenceProperty(c, "m", ["fixtureData"]);
    const before = r.getType(date(0));
    c.add(new C.Entity({ id: "m", fixtureData: { a: 5 } }));
    return [enc(before), r.getType(date(0)), enc(r.getValue(date(0)))];
  });
  reference.add("position", [], () => {
    const c = new C.EntityCollection(), p = new C.ConstantPositionProperty(new C.Cartesian3(...[1, 2, 3])), e = new C.Entity({ id: "p", position: p });
    c.add(e);
    const r = new C.ReferenceProperty(c, "p", ["position"]);
    return [r.referenceFrame, enc(r.getValue(date(0))), enc(r.getValueInReferenceFrame(date(0), 0)), r.equals(new C.ReferenceProperty(c, "p", ["position"])), r.equals(new C.ReferenceProperty(new C.EntityCollection(), "p", ["position"]))];
  });
  out.push(reference);
  return out;
}
export {
  dataSourcesEntities as default
};
