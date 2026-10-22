// Scene foundations: the Scene enums and the small stateless helpers the rest
// of Scene builds on, evaluated by the pinned CesiumJS engine.
import * as C from "@cesium/engine";
import RenderState from "@cesium/engine/Source/Renderer/RenderState.js";
import HeightReference, { isHeightReferenceClamp, isHeightReferenceRelative } from "@cesium/engine/Source/Scene/HeightReference.js";
import Tonemapper, { validateTonemapper } from "@cesium/engine/Source/Scene/Tonemapper.js";
import { suite } from "./lib.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;

// BigInt has no JSON form: `{ bigint: "<decimal>" }`.
function big(v) {
  if (typeof v === "bigint") return { bigint: v.toString() };
  if (Array.isArray(v)) return v.map(big);
  if (v && typeof v === "object" && !ArrayBuffer.isView(v) && Object.getPrototypeOf(v) === Object.prototype) {
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, big(x)]));
  }
  return v;
}
const constants = (e) => big(Object.fromEntries(Object.entries(e).filter(([, v]) => typeof v !== "function")));
// Resolved the way RenderState.fromCache fills in defaults.
function resolved(key, value) {
  const state = RenderState.getState(RenderState.fromCache({ [key]: value }));
  return state[key];
}

// Plain enums: only their constants.
const plain = [
  "ModelAnimationState", "JobType", "TileState", "ArcGisBaseMapType", "Cesium3DTileContentState",
  "Cesium3DTileOptimizationHint", "TerrainState", "ImageryState", "PostProcessStageSampleMode", "MapMode2D",
  "VoxelMetadataOrder", "ExpressionNodeType", "BlendOption", "Cesium3DTileRefine", "CullFace",
  "IonWorldImageryStyle", "LabelStyle", "ModelAnimationLoop", "SplitDirection", "QuadtreeTileLoadState",
  "HorizontalOrigin", "BillboardLoadState", "CameraEventType", "VerticalOrigin", "Cesium3DTileColorBlendMode",
  "BlendEquation", "ResourceLoaderState", "DepthFunction", "StencilFunction", "StencilOperation", "BlendFunction",
  "ClassificationType", "AlphaMode", "BingMapsStyle", "MetadataSemantic", "SDFSettings",
];
const plainModel = [
  ["model", "LightingModel"], ["model", "CustomShaderTranslucencyMode"], ["model", "UniformType"],
  ["model", "VaryingType"], ["model/extensions/gpm", "StorageType"],
];

export default async function sceneFoundations() {
  const out = [];
  const add = (name, dir) => {
    const s = suite(name);
    if (dir) s.dir = dir;
    out.push(s);
    return s;
  };
  const withConstants = async (name, dir) => {
    const e = await scene(dir ? `${dir.replace(/(^|\/)([a-z])/g, (_, p, c) => p + c.toUpperCase())}/${name}` : name);
    const s = add(name, dir);
    s.add("constants", [], () => constants(e));
    return [s, e];
  };

  for (const name of plain) await withConstants(name);
  for (const [dir, name] of plainModel) await withConstants(name, dir);

  {
    const [s, e] = await withConstants("CloudType");
    for (const v of [0, 1, -1, 0.5, -0, 2]) s.add("validate", [v], (v) => e.validate(v));
  }
  {
    const [s, e] = await withConstants("ColorBlendMode");
    for (const mode of [0, 1, 2]) for (const amount of [-1, 0, 1e-5, 1e-4, 0.25, 0.5, 1, 2, NaN, Infinity]) {
      s.add("getColorBlend", [mode, amount], (m, a) => e.getColorBlend(m, a));
    }
  }
  {
    const [s, e] = await withConstants("StencilConstants");
    s.add("setCesium3DTileBit", [], () => e.setCesium3DTileBit());
    s.add("resolved", [], () => resolved("stencilTest", e.setCesium3DTileBit()));
  }
  {
    const [s, e] = await withConstants("ImplicitSubdivisionScheme");
    for (const v of ["QUADTREE", "OCTREE", "quadtree", ""]) s.add("getBranchingFactor", [v], (v) => e.getBranchingFactor(v));
  }
  {
    const [s, e] = await withConstants("Cesium3DTilePass");
    for (let pass = -1; pass <= 8; pass++) s.add("getPassOptions", [pass], (p) => e.getPassOptions(p) ?? null);
  }
  {
    const [s, e] = await withConstants("DynamicAtmosphereLightingType");
    for (let bits = 0; bits < 8; bits++) {
      const globe = { enableLighting: !!(bits & 1), dynamicAtmosphereLighting: !!(bits & 2), dynamicAtmosphereLightingFromSun: !!(bits & 4) };
      s.add("fromGlobeFlags", [globe], (g) => e.fromGlobeFlags(g));
    }
  }
  {
    const [s, e] = await withConstants("SceneMode");
    for (const mode of [0, 1, 2, 3]) s.add("getMorphTime", [mode], (m) => e.getMorphTime(m) ?? null);
  }
  {
    const [s, e] = await withConstants("SensorVolumePortionToDisplay");
    for (const v of [0, 1, 2, 3, -1, 0.5, 4]) s.add("validate", [v], (v) => e.validate(v));
    for (const v of [0, 1, 2, 3]) s.add("toString", [v], (v) => e.toString(v));
  }
  {
    const [s, e] = await withConstants("ShadowMode");
    for (const mode of [0, 1, 2, 3]) s.add("castReceive", [mode], (m) => [e.castShadows(m), e.receiveShadows(m)]);
    for (const cast of [false, true]) for (const receive of [false, true]) {
      s.add("fromCastReceive", [cast, receive], (c, r) => e.fromCastReceive(c, r));
    }
  }
  {
    const [s, e] = await withConstants("TileSelectionResult");
    for (let v = 0; v < 16; v++) s.add("bits", [v], (v) => [e.wasKicked(v), e.originalResult(v), e.kick(v)]);
  }
  {
    const s = add("HeightReference");
    s.add("constants", [], () => constants(HeightReference));
    for (let v = 0; v <= 6; v++) s.add("clampRelative", [v], (v) => [isHeightReferenceClamp(v), isHeightReferenceRelative(v)]);
  }
  {
    const s = add("Tonemapper");
    s.add("constants", [], () => constants(Tonemapper));
    for (const v of ["REINHARD", "MODIFIED_REINHARD", "FILMIC", "ACES", "PBR_NEUTRAL", "reinhard", "", "ACES "]) {
      s.add("validateTonemapper", [v], (v) => validateTonemapper(v));
    }
  }
  const gltfNames = [
    "POSITION", "NORMAL", "TANGENT", "TEXCOORD_0", "TEXCOORD_12", "COLOR_0", "JOINTS_0", "WEIGHTS_1", "_FEATURE_ID_0",
    "_FEATURE_ID", "TRANSLATION", "ROTATION", "SCALE", "SCALE_0", "_SCALE", "_ROTATION", "KHR_gaussian_splatting:SCALE",
    "KHR_gaussian_splatting:ROTATION", "KHR_gaussian_splatting:SCALE_0", "TEXCOORD", "TEXCOORD_", "_0", "__0", "A_1_2",
    "texcoord_0", "TEXCOORD_0a", "TEXCOORD_-1", "POSITION_0", "_FEATURE_ID_", "TÉXCOORD_0", "",
  ];
  {
    const [s, e] = await withConstants("InstanceAttributeSemantic");
    for (const v of gltfNames) s.add("fromGltfSemantic", [v], (v) => e.fromGltfSemantic(v) ?? null);
  }
  {
    const [s, e] = await withConstants("Axis");
    for (const v of ["X", "Y", "Z", "x", "W", ""]) s.add("fromName", [v], (v) => e.fromName(v) ?? null);
  }
  const attributeTypes = ["SCALAR", "VEC2", "VEC3", "VEC4", "MAT2", "MAT3", "MAT4"];
  {
    const [s, e] = await withConstants("AttributeType");
    for (const t of [...attributeTypes, "BOOLEAN"]) {
      s.add("getMathType", [t], (t) => e.getMathType(t).name);
      s.add("getNumberOfComponents", [t], (t) => e.getNumberOfComponents(t));
      s.add("getAttributeLocationCount", [t], (t) => e.getAttributeLocationCount(t));
      s.add("getGlslType", [t], (t) => e.getGlslType(t));
    }
  }
  {
    const [s, e] = await withConstants("MetadataType");
    for (const t of [...attributeTypes, "BOOLEAN", "STRING", "ENUM", "FOO", "vec2"]) {
      s.add("isVectorType", [t], (t) => e.isVectorType(t));
      s.add("isMatrixType", [t], (t) => e.isMatrixType(t));
      s.add("getComponentCount", [t], (t) => e.getComponentCount(t));
      s.add("getMathType", [t], (t) => e.getMathType(t)?.name ?? null);
    }
  }
  {
    const [s, e] = await withConstants("VertexAttributeSemantic");
    const semantics = Object.values(e).filter((v) => typeof v === "string");
    for (const v of [...semantics, "OPACITY"]) {
      s.add("hasSetIndex", [v], (v) => e.hasSetIndex(v));
      s.add("getGlslType", [v], (v) => e.getGlslType(v));
      for (const index of [null, 0, 3]) s.add("getVariableName", [v, index], (v, i) => e.getVariableName(v, i ?? undefined));
    }
    for (const v of gltfNames) s.add("fromGltfSemantic", [v], (v) => e.fromGltfSemantic(v) ?? null);
    for (const v of ["POSITION", "POSITION_QUANTIZED", "RGBA", "RGB", "RGB565", "NORMAL", "NORMAL_OCT16P", "BATCH_ID", "COLOR", "rgb"]) {
      s.add("fromPntsSemantic", [v], (v) => e.fromPntsSemantic(v));
    }
  }
  {
    const [s, e] = await withConstants("Cesium3DTileContentType");
    for (const v of [...Object.values(e).filter((v) => typeof v === "string"), "B3DM", "json", ""]) {
      s.add("isBinaryFormat", [v], (v) => e.isBinaryFormat(v));
    }
  }
  {
    const [s, e] = await withConstants("ModelType", "model");
    for (const v of [...Object.values(e).filter((v) => typeof v === "string"), "TILE_B3DM", "glTF"]) s.add("is3DTiles", [v], (v) => e.is3DTiles(v));
  }
  {
    const [s, e] = await withConstants("CustomShaderMode", "model");
    for (const v of ["MODIFY_MATERIAL", "REPLACE_MATERIAL"]) s.add("getDefineName", [v], (v) => e.getDefineName(v));
  }
  {
    const [s, e] = await withConstants("StyleCommandsNeeded", "model");
    for (const [features, translucent] of [[0, 0], [10, 0], [10, 10], [10, 3], [0, 3], [5, -0], [NaN, NaN], [3, 1.5]]) {
      s.add("getStyleCommandsNeeded", [features, translucent], (f, t) => e.getStyleCommandsNeeded(f, t));
    }
  }
  {
    const [s, e] = await withConstants("BlendingState");
    for (const key of ["DISABLED", "ALPHA_BLEND", "PRE_MULTIPLIED_ALPHA_BLEND", "ADDITIVE_BLEND"]) {
      s.add("resolved", [key], (k) => resolved("blending", e[k]));
    }
  }
  {
    const [s, e] = await withConstants("MetadataComponentType");
    const types = Object.values(e).filter((v) => typeof v === "string");
    for (const t of types) {
      s.add("info", [t], (t) =>
        big([e.getMinimum(t), e.getMaximum(t), e.isIntegerType(t), e.isUnsignedIntegerType(t), e.isVectorCompatible(t), e.category(t), e.getSizeInBytes(t), e.toComponentDatatype(t) ?? null]),
      );
    }
    const numbers = [0, -0, 1, -1, 0.5, -0.5, 2, -2, 1e-20, 0.999999999, 1.0000001, 127, -128, 255, 1e30, NaN, Infinity, -Infinity, 0.123456789];
    for (const t of types) for (const v of numbers) {
      s.add("unnormalize", [v, t], (v, t) => big(e.unnormalize(v, t)));
    }
    const values = [0, 1, -1, 127, -128, 128, 255, 32767, -32768, 65535, 2147483647, -2147483649, 4294967295, 1e19, -1e19, 0.5, NaN];
    const bigints = ["0", "1", "-1", "9223372036854775807", "-9223372036854775808", "18446744073709551615", "9007199254740993", "-12345678901234567890"];
    for (const t of types) {
      for (const v of values) s.add("normalize", [v, t], (v, t) => e.normalize(v, t));
      for (const v of bigints) s.add("normalizeBigInt", [v, t], (v, t) => e.normalize(BigInt(v), t));
    }
    for (const [v, offset, scale] of [[1, 0, 1], [2, 1, 3], [-4, 0.5, 0], [NaN, 0, 1], [1e300, 1e300, 10], [0.1, 0.2, 0.3]]) {
      s.add("applyValueTransform", [v, offset, scale], (v, o, k) => e.applyValueTransform(v, o, k));
      s.add("unapplyValueTransform", [v, offset, scale], (v, o, k) => e.unapplyValueTransform(v, o, k));
    }
    for (const datatype of [5120, 5121, 5122, 5123, 5124, 5125, 5126, 5130, 0, 5131]) {
      s.add("fromComponentDatatype", [datatype], (d) => e.fromComponentDatatype(d) ?? null);
    }
    const writes = [0, 1, -1, 255, 256, -129, 65535, 70000, 2147483648, -2147483649, 4294967297, 0.5, -0.5, 1.5e10, 3.4028235e38, 1e39, NaN, Infinity, 1e20, 2 ** 117, -(2 ** 64) - 2048];
    for (const t of types) {
      for (const v of writes) for (const offset of [0, 3]) {
        s.add("setter", [t, offset, v], (t, offset, v) => {
          const bytes = new Uint8Array(12);
          e.getDataViewSetter(new DataView(bytes.buffer), t)(offset, v);
          return Array.from(bytes);
        });
      }
      for (const v of ["-1", "18446744073709551617", "-9223372036854775809"]) {
        s.add("setterBigInt", [t, 0, v], (t, offset, v) => {
          const bytes = new Uint8Array(12);
          e.getDataViewSetter(new DataView(bytes.buffer), t)(offset, BigInt(v));
          return Array.from(bytes);
        });
      }
      s.add("setter", [t, 10, 1], (t, offset, v) => {
        const bytes = new Uint8Array(12);
        e.getDataViewSetter(new DataView(bytes.buffer), t)(offset, v);
        return Array.from(bytes);
      });
    }
  }
  {
    const s = add("hasExtension");
    const fn = await scene("hasExtension");
    const objects = [null, {}, { extensions: null }, { extensions: {} }, { extensions: { KHR_a: {} } }, { extensions: { KHR_a: null } }, { extensions: { KHR_a: 0 } }, { extensions: { KHR_a: false } }, { extensions: { KHR_b: {} } }, { extensions: [] }];
    for (const json of objects) s.add("hasExtension", [json, "KHR_a"], (j, n) => fn(j ?? undefined, n));
  }
  {
    const s = add("getClipAndStyleCode");
    const fn = await scene("getClipAndStyleCode");
    for (const names of [["clippingPlanes", "clippingPlanesMatrix", "clippingPlanesEdgeStyle"], ["a", "b", "c"], ["", "", ""], ["u_$1", "m{}", "s"]]) {
      s.add("code", names, (a, b, c) => fn(a, b, c));
    }
  }
  {
    const s = add("NeverTileDiscardPolicy");
    const Policy = await scene("NeverTileDiscardPolicy");
    s.add("policy", [], () => {
      const p = new Policy();
      return [p.isReady(), p.shouldDiscardImage({}), p.shouldDiscardImage(undefined)];
    });
  }
  {
    const s = add("Splitter");
    const Splitter = await scene("Splitter");
    for (const shader of ["void main() { out_FragColor = vec4(1.0); }", "void main(void)\n{\n}\n", "uniform float x;\nvoid  main ( ) {}\nvoid helper() {}", "void helper() {}"]) {
      s.add("modifyFragmentShader", [shader], (v) => Splitter.modifyFragmentShader(v));
    }
    for (const direction of [-1, 0, 1]) {
      s.add("addUniforms", [direction], (d) => {
        const object = { splitDirection: d };
        const map = { u_other: () => 7 };
        Splitter.addUniforms(object, map);
        return [Object.keys(map), map.czm_splitDirection()];
      });
    }
  }
  {
    const s = add("SupportedImageFormats");
    const Formats = await scene("SupportedImageFormats");
    for (const options of [null, {}, { webp: true }, { basis: true, webp: false }]) {
      s.add("new", [options], (o) => {
        const f = new Formats(o ?? undefined);
        return [f.webp, f.basis];
      });
    }
  }
  {
    const s = add("SunLight");
    for (const options of [null, {}, { color: new C.Color(1, 0.5, 0.25, 1) }, { intensity: 0.5 }, { color: C.Color.RED, intensity: 0 }]) {
      s.add("new", [options], (o) => {
        const light = new C.SunLight(o ?? undefined);
        return [light.color, light.intensity];
      });
    }
  }
  {
    const s = add("DirectionalLight");
    for (const options of [{ direction: new C.Cartesian3(1, 0, 0) }, { direction: new C.Cartesian3(0.2, -0.3, 0.9), color: C.Color.BLUE, intensity: 3 }, { direction: new C.Cartesian3(0, 0, 0) }, { direction: new C.Cartesian3(-0, 0, 0) }]) {
      s.add("new", [options], (o) => {
        const light = new C.DirectionalLight(o);
        return [light.direction, light.color, light.intensity];
      });
    }
  }
  {
    const s = add("ParticleBurst");
    for (const options of [null, {}, { time: 5, minimum: 10, maximum: 100 }, { maximum: 0 }]) {
      s.add("new", [options], (o) => {
        const burst = new C.ParticleBurst(o ?? undefined);
        return [burst.time, burst.minimum, burst.maximum, burst.complete];
      });
    }
  }
  {
    const s = add("QuadtreeOccluders");
    const Occluders = await scene("QuadtreeOccluders");
    for (const name of ["WGS84", "UNIT_SPHERE", "MOON"]) {
      s.add("new", [name], (n) => {
        const occluders = new Occluders({ ellipsoid: C.Ellipsoid[n] });
        return [occluders.ellipsoid.ellipsoid.radii, occluders.ellipsoid.cameraPosition];
      });
    }
  }
  {
    const s = add("getBinaryAccessor");
    const fn = await scene("getBinaryAccessor");
    const bytes = Array.from({ length: 64 }, (_, i) => (i * 37 + 11) & 255);
    const cases = [["FLOAT", "VEC3", 0, 2], ["UNSIGNED_SHORT", "SCALAR", 2, 5], [5121, "VEC4", 1, 3], ["DOUBLE", "MAT2", 0, 1], ["BYTE", "MAT3", 3, 2], ["INT", "SCALAR", 4, 0], ["UNSIGNED_INT", "VEC2", 8, 3], ["FLOAT", "VEC3", 60, 2]];
    for (const [componentType, type, offset, length] of cases) {
      s.add("accessor", [componentType, type, offset, length, bytes], (c, t, o, l, b) => {
        const accessor = fn({ componentType: c, type: t });
        const view = accessor.createArrayBufferView(Uint8Array.from(b).buffer, o, l);
        return [accessor.componentsPerAttribute, accessor.classType?.name ?? null, Array.from(view)];
      });
    }
  }
  {
    const s = add("ModelAlphaOptions", "model");
    const Options = await scene("Model/ModelAlphaOptions");
    s.add("new", [], () => {
      const o = new Options();
      return [o.pass ?? null, o.alphaCutoff ?? null];
    });
  }
  {
    const s = add("ModelLightingOptions", "model");
    const Options = await scene("Model/ModelLightingOptions");
    for (const options of [null, {}, { lightingModel: 1 }, { lightingModel: 0 }]) {
      s.add("new", [options], (o) => new Options(o ?? undefined).lightingModel);
    }
  }
  {
    const s = add("ImageryFlags", "model");
    const Flags = await scene("Model/ImageryFlags");
    s.add("new", [], () => ({ ...new Flags() }));
  }
  {
    const s = add("CartesianRectangle", "model");
    const Rectangle = await scene("Model/CartesianRectangle");
    s.add("defaults", [], () => {
      const r = new Rectangle();
      return [r.minX, r.minY, r.maxX, r.maxY];
    });
    const points = [[0, 0], [1, 1], [0.5, 0.5], [1, 0.5], [0.5, 1], [-0.1, 0.5], [2, 2], [NaN, 0]];
    for (const [x, y] of points) {
      s.add("contains", [0, 0, 1, 1, x, y], (a, b, c, d, x, y) => {
        const r = new Rectangle(a, b, c, d);
        return [r.contains(x, y), r.containsExclusive(x, y), r.containsInclusive(x, y)];
      });
    }
  }
  return out;
}
