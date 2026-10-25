import * as C from "@cesium/engine";
import { FixtureComponent } from "./data-sources-entities.mjs";
import { suite, enc } from "./lib.mjs";
const date = (t) => C.JulianDate.addSeconds(C.JulianDate.fromIso8601("2020-01-01T00:00:00Z"), t, new C.JulianDate());
const availability = (v) => new C.TimeIntervalCollection([new C.TimeInterval({ start: date(v[0]), stop: date(v[1]) })]);
function entity(spec) {
  const options = { ...spec };
  delete options.custom;
  if (options.position) options.position = new C.Cartesian3(...options.position);
  if (options.availability) options.availability = availability(options.availability);
  const e = new C.Entity(options);
  if ("custom" in spec) {
    e.addProperty("custom");
    e.custom = spec.custom;
  }
  return e;
}
function value(e, name) {
  const v = e[name];
  return v?.getValue ? enc(v.getValue(date(0))) : enc(v);
}
function capture(c, layers, trace, identities) {
  const span = c.computeAvailability();
  return [
    Array.from({ length: c.getCollectionsLength() }, (_, i) => layers.indexOf(c.getCollection(i))),
    c.values.map((e) => {
      const same = !identities.has(e.id) || identities.get(e.id) === e;
      identities.set(e.id, e);
      return [e.id, e.name ?? null, e.propertyNames.filter((n) => !["fixtureData"].includes(n) && !baseNames.includes(n)), value(e, "description"), value(e, "fixtureData"), value(e, "properties"), value(e, "custom"), value(e, "position"), e.availability ? [C.JulianDate.toIso8601(e.availability.start), C.JulianDate.toIso8601(e.availability.stop)] : null, same, c.contains(e)];
    }),
    trace.splice(0),
    [C.JulianDate.toIso8601(span.start), C.JulianDate.toIso8601(span.stop)],
    layers.map((l) => [l.collectionChanged.numberOfListeners, l.values.map((e) => [e.id, e.definitionChanged.numberOfListeners])])
  ];
}
const baseNames = ["billboard", "box", "corridor", "cylinder", "description", "ellipse", "ellipsoid", "label", "model", "tileset", "orientation", "path", "plane", "point", "polygon", "polyline", "polylineVolume", "position", "properties", "rectangle", "viewFrom", "wall"];
function dataSourcesComposite() {
  if (!Object.getOwnPropertyDescriptor(C.Entity.prototype, "fixtureData")) C.Entity.registerEntityType("fixtureData", FixtureComponent);
  const out = suite("CompositeEntityCollection");
  const run = (scenario) => {
    const layers2 = scenario.layers.map((specs) => {
      const l = new C.EntityCollection();
      for (const spec of specs) l.add(entity(spec));
      return l;
    });
    const c = new C.CompositeEntityCollection(scenario.order.map((i) => layers2[i]));
    const trace = [], identities = /* @__PURE__ */ new Map();
    c.collectionChanged.addEventListener((_, a, r, h) => trace.push([a.map((e) => e.id), r.map((e) => e.id), h.map((e) => e.id)]));
    const snapshots = [capture(c, layers2, trace, identities)];
    for (const op of scenario.ops) {
      const [kind, i, id, key, v] = op, l = layers2[i];
      if (kind === "addCollection") c.addCollection(l, id ?? void 0);
      if (kind === "removeCollection") c.removeCollection(l);
      if (kind === "allCollections") c.removeAllCollections();
      if (kind === "raise") c.raiseCollection(l);
      if (kind === "lower") c.lowerCollection(l);
      if (kind === "top") c.raiseCollectionToTop(l);
      if (kind === "bottom") c.lowerCollectionToBottom(l);
      if (kind === "suspend") c.suspendEvents();
      if (kind === "resume") c.resumeEvents();
      if (kind === "add") l.add(entity(id));
      if (kind === "remove") l.removeById(id);
      if (kind === "clear") l.removeAll();
      if (kind === "set") {
        let next = v === null ? void 0 : v;
        if (key === "position" && next) next = new C.Cartesian3(...next);
        if (key === "availability" && next) next = availability(next);
        l.getById(id)[key] = next;
      }
      if (kind === "child") l.getById(id)[key.split(".")[0]][key.split(".")[1]] = v === null ? void 0 : v;
      if (kind === "mutate") l.getById(id)[key].setValue(v);
      if (kind === "custom") {
        l.getById(id).addProperty(key);
        l.getById(id)[key] = v;
      }
      snapshots.push(capture(c, layers2, trace, identities));
    }
    return snapshots;
  };
  const layers = [
    [{ id: "x", name: "low", description: "low", fixtureData: { a: 1 }, custom: 1, availability: [0, 10] }, { id: "lowOnly", description: "L" }],
    [{ id: "x", name: "middle", description: "middle", fixtureData: { b: 2 }, position: [1, 2, 3] }, { id: "midOnly", description: "M" }],
    [{ id: "x", name: "high", description: "high", fixtureData: { a: 3 }, availability: [-2, 20] }, { id: "highOnly", description: "H" }]
  ];
  for (const order of [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]]) {
    for (const moved of [0, 1, 2]) out.add("operations", [{ layers, order, ops: [["raise", moved], ["lower", moved], ["top", moved], ["bottom", moved], ["removeCollection", moved], ["addCollection", moved, 1], ["allCollections"]] }], run);
    for (const depth of [0, 1, 2]) {
      const ops = [...Array.from({ length: depth }, () => ["suspend"]), ["set", 2, "x", "description", null], ["set", 0, "x", "description", "changed"], ["child", 1, "x", "fixtureData.b", 5], ["set", 2, "x", "fixtureData", null], ["remove", 2, "x"], ["remove", 1, "x"], ["add", 2, { id: "x", description: "replacement", fixtureData: { b: 9 } }], ["clear", 0], ...Array.from({ length: depth }, () => ["resume"])];
      out.add("operations", [{ layers, order, ops }], run);
    }
  }
  for (const order of [[], [0], [1], [0, 1], [1, 0]]) out.add("operations", [{ layers: [[], []], order, ops: [["add", 0, { id: "x", description: "first" }], ["add", 1, { id: "x", description: "second" }], ["removeCollection", 0], ["addCollection", 0], ["remove", 1, "x"], ["clear", 0], ["allCollections"]] }], run);
  for (const initial of [true, false]) out.add("operations", [{ layers: [[{ id: "x", fixtureData: { a: 1 } }], [{ id: "x", fixtureData: initial ? { b: 2 } : void 0 }]], order: [0, 1], ops: [["child", 0, "x", "fixtureData.a", 4], ["set", 1, "x", "fixtureData", { a: 5 }], ["set", 1, "x", "fixtureData", null], ["set", 0, "x", "fixtureData", null]] }], run);
  out.add("operations", [{ layers: [[{ id: "x", properties: { a: 1 } }], [{ id: "x", properties: { a: 2, b: 3 } }]], order: [0, 1], ops: [["child", 1, "x", "properties.a", 4], ["set", 1, "x", "properties", null]] }], run);
  out.add("operations", [{ layers: [[{ id: "x", name: "first", description: "first", availability: [0, 10] }]], order: [0], ops: [["set", 0, "x", "name", "next"], ["mutate", 0, "x", "description", "second"], ["set", 0, "x", "availability", [20, 30]], ["set", 0, "x", "position", [4, 5, 6]], ["custom", 0, "x", "custom", 3]] }], run);
  return [out];
}
export {
  dataSourcesComposite as default
};
