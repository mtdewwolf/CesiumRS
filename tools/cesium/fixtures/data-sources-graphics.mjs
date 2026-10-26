import * as C from "@cesium/engine";
import { suite, enc } from "./lib.mjs";

const epoch = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const date = (t) => C.JulianDate.addSeconds(epoch, t, new C.JulianDate());

const GraphicsTypes = {
  BillboardGraphics: C.BillboardGraphics,
  BoxGraphics: C.BoxGraphics,
  Cesium3DTilesetGraphics: C.Cesium3DTilesetGraphics,
  CorridorGraphics: C.CorridorGraphics,
  CylinderGraphics: C.CylinderGraphics,
  EllipseGraphics: C.EllipseGraphics,
  EllipsoidGraphics: C.EllipsoidGraphics,
  LabelGraphics: C.LabelGraphics,
  ModelGraphics: C.ModelGraphics,
  PathGraphics: C.PathGraphics,
  PlaneGraphics: C.PlaneGraphics,
  PointGraphics: C.PointGraphics,
  PolygonGraphics: C.PolygonGraphics,
  PolylineGraphics: C.PolylineGraphics,
  PolylineVolumeGraphics: C.PolylineVolumeGraphics,
  RectangleGraphics: C.RectangleGraphics,
  WallGraphics: C.WallGraphics,
};

const propertyNames = (Type) => Object.keys(Object.getOwnPropertyDescriptors(Type.prototype)).filter((name) => {
  const descriptor = Object.getOwnPropertyDescriptor(Type.prototype, name);
  return descriptor && typeof descriptor.get === "function" && name !== "definitionChanged";
});

function describe(value) {
  if (value === undefined) return ["undefined"];
  if (value && typeof value.getValue === "function") {
    return ["property", enc(value.getValue(date(0)))];
  }
  return ["raw", enc(value)];
}

function capture(g, trace) {
  return [
    propertyNames(g.constructor).map((name) => [
      name,
      describe(g[name]),
      g[name] && g[name].definitionChanged ? g[name].definitionChanged.numberOfListeners : 0,
    ]),
    trace.splice(0),
  ];
}

function makeValue(g, name, index, pool) {
  if (name === "material" || name === "depthFailMaterial") {
    return new C.Color(0.01 * index, 0.2, 0.3, 0.4);
  }
  if (index % 2 === 0) return index + 0.25;
  const id = `p${index}`;
  if (!pool.has(id)) pool.set(id, new C.ConstantProperty(index + 0.75));
  return pool.get(id);
}

function unsupportedNames(Type) {
  // Model graphics node/articulation raw-object coercion needs
  // NodeTransformationProperty and PropertyBag factories, which are not part
  // of this batch. Their direct property assignment is covered natively.
  return Type === C.ModelGraphics ? ["nodeTransformations", "articulations"] : [];
}

function supportedNames(Type) {
  const unsupported = unsupportedNames(Type);
  return propertyNames(Type).filter((name) => !unsupported.includes(name));
}

function optionsFor(Type, pool) {
  const options = {};
  supportedNames(Type).forEach((name, index) => {
    options[name] = makeValue(Type.prototype, name, index, pool);
  });
  return options;
}

function graphicsDefinition(Type) {
  const pool = new Map();
  let trace = [];
  const g = new Type(optionsFor(Type, pool));
  g.definitionChanged.addEventListener((owner, name, value, old) => trace.push([name, describe(value), describe(old), value === old]));
  const out = [capture(g, trace)];

  // Reassigning a raw slot creates or shares the supplied property object and
  // emits a definition event; number-keyed order is the descriptor order.
  const names = supportedNames(Type);
  const q = new C.ConstantProperty(99);
  pool.set("q", q);
  names.forEach((name) => (g[name] = q));
  out.push(capture(g, trace));

  // Child definition changes forward with old === new.
  q.setValue(100);
  out.push(capture(g, trace)); // [A]

  // clone() without a result routes through the constructor's merge path.
  const cloned = g.clone();
  out.push(capture(cloned, [])); // [B]

  // clone(result) overwrites every upstream clone field. ModelGraphics omits
  // shadows here, so a pre-set result keeps its old shadows value.
  const target = new Type();
  target.shadows = q;
  g.clone(target);
  out.push(capture(target, [])); // [C]

  // merge only fills missing slots and shares property objects.
  const preset = new Type();
  preset.show = q;
  preset.merge(g);
  out.push(capture(preset, [])); // [D]

  // Clearing then merging the original restores the source fields.
  names.forEach((name) => (g[name] = undefined));
  out.push(capture(g, trace));
  g.merge(cloned);
  out.push(capture(g, trace));

  // Entity-style construction from a raw options object.
  const fromOptions = new Type(optionsFor(Type, new Map()));
  out.push(capture(fromOptions, []));

  return out;
}


function polygonHierarchy() {
  const positions = [new C.Cartesian3(1, 2, 3), new C.Cartesian3(4, 5, 6)];
  const fromArray = new C.PolygonGraphics({ hierarchy: positions });
  const fromObject = new C.PolygonGraphics({ hierarchy: { positions, holes: [] } });
  const direct = new C.PolygonGraphics();
  direct.hierarchy = positions;
  const wrapped = new C.PolygonGraphics();
  wrapped.hierarchy = new C.ConstantProperty(positions);
  return [capture(fromArray, []), capture(fromObject, []), capture(direct, []), capture(wrapped, [])];
}

function modelBags() {
  const raw = new C.ModelGraphics({
    articulations: { "arm bend": 2 },
    environmentMapOptions: { specular: 0.5 },
  });
  const source = new C.ModelGraphics({
    articulations: { "arm bend": 3 },
    environmentMapOptions: { specular: 0.7 },
  });
  const target = new C.ModelGraphics();
  target.merge(source);
  return [capture(raw, []), capture(target, [])];
}

function colorMaterial() {
  const p = new C.ColorMaterialProperty(new C.Color(0.25, 0.5, 0.75, 1.0));
  const trace = [];
  p.definitionChanged.addEventListener((owner, name, value, old) => trace.push([name, describe(value), describe(old), value === old]));
  const out = [[p.getType(date(0)), enc(p.getValue(date(0))), trace.splice(0)]];
  const color = new C.ConstantProperty(new C.Color(1, 0, 0, 0.5));
  p.color = color;
  out.push([p.isConstant, enc(p.getValue(date(0))), trace.splice(0)]);
  color.setValue(new C.Color(0, 1, 0, 1));
  out.push([p.isConstant, enc(p.getValue(date(0))), trace.splice(0)]);
  const other = new C.ColorMaterialProperty(new C.Color(1, 0, 0, 0.5));
  out.push([
    p.equals(p),
    p.equals(other),
    p.equals(new C.ColorMaterialProperty(new C.Color(0, 1, 0, 1))),
    describe(p.color),
  ]);
  return out;
}

export default function dataSourcesGraphics() {
  const s = suite("Graphics");
  for (const name of Object.keys(GraphicsTypes)) {
    s.add("definition", [name], (name2) => graphicsDefinition(GraphicsTypes[name2]));
  }
  s.add("polygonHierarchy", [], () => polygonHierarchy());
  s.add("modelBags", [], () => modelBags());
  s.add("colorMaterial", [], () => colorMaterial());
  return [s];
}
