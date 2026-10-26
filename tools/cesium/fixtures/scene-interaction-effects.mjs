// Primitives, appearances and materials evaluated by the pinned CesiumJS
// engine: generated GLSL, uniform renames, render states and clipping
// polygons.
import { suite } from "./lib.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;
const core = async (name) => (await import(`@cesium/engine/Source/Core/${name}.js`)).default;

export default async function sceneInteractionEffects() {
  // Cesium's `getUniformType` uses `instanceof` on DOM classes; Node has none.
  // Empty placeholders make those checks false, as they are for strings in a browser.
  for (const name of ["HTMLCanvasElement", "HTMLImageElement", "ImageBitmap", "OffscreenCanvas", "HTMLVideoElement"]) {
    if (!(name in globalThis)) globalThis[name] = class {};
  }
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };

  const Cartesian2 = await core("Cartesian2");
  const Cartesian3 = await core("Cartesian3");
  const Color = await core("Color");
  const Ellipsoid = await core("Ellipsoid");
  const VertexFormat = await core("VertexFormat");
  const Material = await scene("Material");
  const Appearance = await scene("Appearance");
  const MaterialAppearance = await scene("MaterialAppearance");
  const EllipsoidSurfaceAppearance = await scene("EllipsoidSurfaceAppearance");
  const PerInstanceColorAppearance = await scene("PerInstanceColorAppearance");
  const PolylineColorAppearance = await scene("PolylineColorAppearance");
  const PolylineMaterialAppearance = await scene("PolylineMaterialAppearance");
  const DebugAppearance = await scene("DebugAppearance");
  const PrimitiveState = await scene("PrimitiveState");
  const ClippingPolygon = await scene("ClippingPolygon");
  const ClippingPolygonCollection = await scene("ClippingPolygonCollection");
  const getClippingFunction = await scene("getClippingFunction");
  const resultLimits = (await import("@cesium/engine/Source/Renderer/ContextLimits.js")).default;

  const vf = (v) => [v.position, v.normal, v.st, v.tangent, v.bitangent, v.color];
  const color = (c) => [c.red, c.green, c.blue, c.alpha];
  const renderState = (rs) => ({
    depthMask: rs.depthMask,
    depthTest: rs.depthTest?.enabled,
    blending: {
      enabled: rs.blending?.enabled,
      equationRgb: rs.blending?.equationRgb,
      sourceRgb: rs.blending?.functionSourceRgb,
      destinationRgb: rs.blending?.functionDestinationRgb,
    },
    cull: { enabled: rs.cull?.enabled, face: rs.cull?.face },
  });
  const uniformValue = (v) => {
    if (typeof v === "function") return { function: true };
    return enc(v);
  };
  const enc = (v) => {
    if (v === undefined || v === null) return null;
    if (typeof v === "number" || typeof v === "boolean" || typeof v === "string") return v;
    if (v instanceof Color) return color(v);
    if (v instanceof Cartesian2) return [v.x, v.y];
    if (v instanceof Cartesian3) return [v.x, v.y, v.z];
    if (Array.isArray(v) || ArrayBuffer.isView(v)) return Array.from(v);
    if (typeof v === "object") {
      const o = {};
      for (const [k, x] of Object.entries(v)) o[k] = enc(x);
      return o;
    }
    return String(v);
  };
  const materialState = (m) => ({
    type: m.type,
    shaderSource: m.shaderSource,
    materials: Object.keys(m.materials),
    uniforms: Object.entries(m.uniforms).map(([k, v]) => [k, uniformValue(v)]),
    // `_uniforms` maps the renamed name to a getter; the names are what the
    // renderer binds, the values live in `uniforms`.
    uniformBindings: Object.keys(m._uniforms),
    translucent: m.isTranslucent(),
  });

  // ---- Material ----
  {
    const s = add("Material");
    const types = [
      "Color",
      "Image",
      "DiffuseMap",
      "AlphaMap",
      "SpecularMap",
      "EmissionMap",
      "BumpMap",
      "NormalMap",
      "Grid",
      "Stripe",
      "Checkerboard",
      "Dot",
      "Water",
      "RimLighting",
      "Fade",
      "PolylineArrow",
      "PolylineDash",
      "PolylineGlow",
      "PolylineOutline",
      "ElevationContour",
      "ElevationRamp",
      "SlopeRamp",
      "AspectRamp",
      "ElevationBand",
      "WaterMask",
    ];
    for (const type of types) {
      s.add("fromType", [type], (t) => materialState(Material.fromType(t)));
    }
    s.add("fromTypeUniforms", [["Color", [0.25, 0.5, 0.75, 0.4]]], ([type, c]) =>
      materialState(Material.fromType(type, { color: new Color(c[0], c[1], c[2], c[3]) })),
    );
    s.add("imageUniform", [], () =>
      materialState(Material.fromType("Image", { image: Material.DefaultImageId, repeat: new Cartesian2(2, 3) })),
    );
    s.add("customComponents", [], () =>
      materialState(
        new Material({
          fabric: {
            type: "TestComponents",
            uniforms: { color: new Color(1, 0, 0, 1), amount: 0.5 },
            components: { diffuse: "color.rgb * amount", alpha: "color.a" },
          },
        }),
      ),
    );
    s.add("customSource", [], () =>
      materialState(
        new Material({
          fabric: {
            type: "TestSource",
            uniforms: { color: new Color(0, 1, 0, 1) },
            source:
              "czm_material czm_getMaterial(czm_materialInput materialInput)\n{\nczm_material material = czm_getDefaultMaterial(materialInput);\nmaterial.diffuse = color.rgb;\nreturn material;\n}\n",
          },
        }),
      ),
    );
    s.add("subMaterial", [], () =>
      materialState(
        new Material({
          fabric: {
            type: "TestSub",
            materials: {
              sub: {
                type: "Color",
                uniforms: { color: new Color(1, 0, 0, 1) },
              },
            },
            uniforms: { extent: 3 },
            components: { diffuse: "sub.diffuse * extent", alpha: "sub.alpha" },
          },
        }),
      ),
    );
    s.add("subMaterialSource", [], () =>
      materialState(
        new Material({
          fabric: {
            type: "TestSubSource",
            materials: {
              noise: {
                source:
                  "czm_material czm_getMaterial(czm_materialInput materialInput)\n{\nczm_material material = czm_getDefaultMaterial(materialInput);\nmaterial.diffuse = vec3(0.0);\nreturn material;\n}\n",
                uniforms: { frequency: 10 },
              },
            },
            uniforms: { color: new Color(0, 0, 1, 1) },
            source:
              "czm_material czm_getMaterial(czm_materialInput materialInput)\n{\nczm_material material = czm_getDefaultMaterial(materialInput);\nmaterial.diffuse = noise.diffuse + color.rgb;\nreturn material;\n}\n",
          },
        }),
      ),
    );
    s.add("translucentOverride", [], () => Material.fromType("Color", { color: new Color(1, 1, 1, 0.2) }).isTranslucent());
    s.add("setUniform", [], () => {
      const m = Material.fromType("Color");
      m.uniforms.color = new Color(0, 1, 0, 0.3);
      return { translucent: m.isTranslucent(), uniforms: m.uniforms.color && color(m.uniforms.color) };
    });
    s.add("unknown", [], () => Material.fromType("NotAType"));
  }

  // ---- Appearance ----
  {
    const s = add("Appearance");
    const defaults = [
      [true, false, null],
      [false, false, null],
      [true, true, null],
      [false, true, null],
      [true, false, { depthMask: true, cull: { enabled: true, face: 1029 } }],
    ];
    for (const [translucent, closed, existing] of defaults) {
      s.add("getDefaultRenderState", [translucent, closed, existing], (t, c, e) =>
        renderState(Appearance.getDefaultRenderState(t, c, e)),
      );
    }
  }

  const appearanceCase = (s, name, make, extra = () => ({})) => {
    s.add(name, [], () => {
      const a = make();
      return {
        vertexShaderSource: a.vertexShaderSource,
        fragmentShaderSource: a.fragmentShaderSource,
        getFragmentShaderSource: a.getFragmentShaderSource(),
        renderState: renderState(a.renderState),
        getRenderState: renderState(a.getRenderState()),
        isTranslucent: a.isTranslucent(),
        closed: a.closed,
        flat: a.flat,
        faceForward: a.faceForward,
        vertexFormat: a.vertexFormat ? vf(a.vertexFormat) : null,
        materialType: a.material?.type,
        ...extra(a),
      };
    });
  };

  {
    const s = add("MaterialAppearance");
    appearanceCase(s, "default", () => new MaterialAppearance());
    appearanceCase(s, "basic", () => new MaterialAppearance({ materialSupport: MaterialAppearance.MaterialSupport.BASIC, material: Material.fromType("Color") }));
    appearanceCase(s, "all", () => new MaterialAppearance({ materialSupport: MaterialAppearance.MaterialSupport.ALL, material: Material.fromType("Stripe") }));
    appearanceCase(s, "closed", () => new MaterialAppearance({ closed: true, material: Material.fromType("Color") }));
    appearanceCase(s, "flatFaceForward", () => new MaterialAppearance({ flat: true, faceForward: false, material: Material.fromType("Grid") }));
    appearanceCase(s, "opaque", () => new MaterialAppearance({ translucent: false, material: Material.fromType("Color") }));
    s.add("supports", [], () =>
      Object.keys(MaterialAppearance.MaterialSupport).map((k) => {
        const v = MaterialAppearance.MaterialSupport[k];
        return { key: k, vertexFormat: vf(v.vertexFormat), vs: v.vertexShaderSource, fs: v.fragmentShaderSource };
      }),
    );
  }
  {
    const s = add("EllipsoidSurfaceAppearance");
    appearanceCase(s, "default", () => new EllipsoidSurfaceAppearance());
    appearanceCase(s, "aboveGround", () => new EllipsoidSurfaceAppearance({ aboveGround: true }));
    appearanceCase(s, "flat", () => new EllipsoidSurfaceAppearance({ flat: true, material: Material.fromType("Stripe") }));
    appearanceCase(s, "opaque", () => new EllipsoidSurfaceAppearance({ translucent: false }));
  }
  {
    const s = add("PerInstanceColorAppearance");
    appearanceCase(s, "default", () => new PerInstanceColorAppearance());
    appearanceCase(s, "flat", () => new PerInstanceColorAppearance({ flat: true }));
    appearanceCase(s, "closed", () => new PerInstanceColorAppearance({ closed: true }));
    appearanceCase(s, "opaque", () => new PerInstanceColorAppearance({ translucent: false }));
  }
  {
    const s = add("PolylineColorAppearance");
    appearanceCase(s, "default", () => new PolylineColorAppearance());
    appearanceCase(s, "opaque", () => new PolylineColorAppearance({ translucent: false }));
  }
  {
    const s = add("PolylineMaterialAppearance");
    appearanceCase(s, "default", () => new PolylineMaterialAppearance());
    appearanceCase(s, "glow", () => new PolylineMaterialAppearance({ material: Material.fromType("PolylineGlow") }));
    appearanceCase(s, "dashDefine", () =>
      new PolylineMaterialAppearance({
        material: new Material({
          fabric: {
            type: "DashTest",
            source: "in float v_polylineAngle;\nczm_material czm_getMaterial(czm_materialInput materialInput)\n{\nreturn czm_getDefaultMaterial(materialInput);\n}\n",
          },
        }),
      }),
    );
  }
  {
    const s = add("DebugAppearance");
    appearanceCase(s, "normal", () => new DebugAppearance({ attributeName: "normal" }));
    appearanceCase(s, "st", () => new DebugAppearance({ attributeName: "st" }));
    appearanceCase(s, "customVec4", () => new DebugAppearance({ attributeName: "temperature", glslDatatype: "vec4" }));
    appearanceCase(s, "perInstance", () => new DebugAppearance({ attributeName: "color", perInstanceAttribute: true, glslDatatype: "vec4" }));
    s.add("missing", [], () => new DebugAppearance({}));
    s.add("badDatatype", [], () => new DebugAppearance({ attributeName: "a", glslDatatype: "mat3" }));
  }

  // ---- PrimitiveState ----
  {
    const s = add("PrimitiveState");
    s.add("values", [], () => ({ ...PrimitiveState }));
  }

  // ---- ClippingPolygon ----
  const ring = (west, south, east, north) =>
    Cartesian3.fromRadiansArray([west, south, east, south, east, north, west, north, west, south]);
  const polygons = [
    ring(-1.32, 0.69, -1.3, 0.71),
    ring(0.4, -0.3, 0.5, -0.2),
    ring(-1.31, 0.695, -1.29, 0.705),
  ];
  {
    const s = add("ClippingPolygon");
    for (const positions of polygons) {
      s.add("rectangle", [positions], (p) => {
        const polygon = new ClippingPolygon({ positions: p });
        const r = polygon.computeRectangle();
        return [r.west, r.south, r.east, r.north];
      });
      s.add("spherical", [positions], (p) => {
        const polygon = new ClippingPolygon({ positions: p });
        const r = polygon.computeSphericalExtents();
        return [r.west, r.south, r.east, r.north];
      });
    }
    s.add("length", [polygons[0]], (p) => new ClippingPolygon({ positions: p }).length);
    s.add("ellipsoid", [], () => new ClippingPolygon({ positions: polygons[0] }).ellipsoid.equals(Ellipsoid.WGS84));
    s.add("equals", [polygons[0]], (a) => {
      const pa = new ClippingPolygon({ positions: a });
      return {
        self: ClippingPolygon.equals(pa, pa),
        copy: ClippingPolygon.equals(pa, ClippingPolygon.clone(pa)),
        other: ClippingPolygon.equals(pa, new ClippingPolygon({ positions: a.slice() })),
      };
    });
    s.add("tooFew", [[]], () => new ClippingPolygon({ positions: [] }));
  }
  {
    const s = add("ClippingPolygonCollection");
    const makeCollection = (positions, options = {}) =>
      new ClippingPolygonCollection({ polygons: positions.map((p) => new ClippingPolygon({ positions: p })), ...options });
    s.add("empty", [], () => {
      const c = new ClippingPolygonCollection();
      return {
        length: c.length,
        totalPositions: c.totalPositions,
        extentsCount: c.extentsCount,
        state: c.clippingPolygonsState,
        pixelsExtents: c.pixelsNeededForExtents,
        pixelsPositions: c.pixelsNeededForPolygonPositions,
        enabled: c.enabled,
        inverse: c.inverse,
        owner: c.owner,
      };
    });
    s.add("counts", [polygons.slice(0, 2)], (ps) => {
      const c = makeCollection(ps);
      return {
        length: c.length,
        totalPositions: c.totalPositions,
        pixelsExtents: c.pixelsNeededForExtents,
        pixelsPositions: c.pixelsNeededForPolygonPositions,
        state: c.clippingPolygonsState,
        contains: c.contains(new ClippingPolygon({ positions: ps[0] })),
      };
    });
    s.add("inverse", [polygons.slice(0, 2)], (ps) => makeCollection(ps, { inverse: true }).clippingPolygonsState);
    s.add("events", [polygons], (ps) => {
      const c = makeCollection(ps);
      const added = [];
      const removed = [];
      c.polygonAdded.addEventListener((polygon, index) => added.push([index, polygon.length]));
      c.polygonRemoved.addEventListener((polygon, index) => removed.push([index, polygon.length]));
      const p = new ClippingPolygon({ positions: ps[2] });
      c.add(p);
      const contains = c.contains(p);
      const didRemove = c.remove(c.get(0));
      c.removeAll();
      return { added, removed, contains, didRemove, length: c.length };
    });
    s.add("textureResolution", [3, 5], (pixels, pixels2) => {
      const ContextLimits = resultLimits;
      const previous = ContextLimits._maximumTextureSize;
      ContextLimits._maximumTextureSize = 2048;
      const one = ClippingPolygonCollection.getTextureResolution(undefined, pixels, new Cartesian2());
      const two = ClippingPolygonCollection.getTextureResolution(undefined, pixels2, new Cartesian2());
      const big = ClippingPolygonCollection.getTextureResolution(undefined, 5000, new Cartesian2());
      ContextLimits._maximumTextureSize = previous;
      return [[one.x, one.y], [two.x, two.y], [big.x, big.y]];
    });
    s.add("supported", [], () => [
      ClippingPolygonCollection.isSupported({ context: { webgl2: true } }),
      ClippingPolygonCollection.isSupported({ context: { webgl2: false } }),
      ClippingPolygonCollection.isSupported(undefined),
    ]);
  }

  // ---- getClippingFunction ----
  {
    const s = add("getClippingFunction");
    const fake = (length, union, texture) => ({ length, unionClippingRegions: union, texture });
    const cases = [
      [fake(3, false, { width: 3, height: 2 }), { floatingPointTexture: true }],
      [fake(3, true, { width: 3, height: 2 }), { floatingPointTexture: true }],
      [fake(2, false, { width: 4, height: 2 }), { floatingPointTexture: false }],
      [fake(1, true, { width: 2, height: 2 }), { floatingPointTexture: false }],
      [fake(0, false, { width: 1, height: 1 }), { floatingPointTexture: true }],
    ];
    for (const [collection, context] of cases) {
      s.add("function", [collection.length, collection.unionClippingRegions, collection.texture.width, collection.texture.height, context.floatingPointTexture], (length, union, width, height, float) =>
        getClippingFunction(fake(length, union, { width, height }), { floatingPointTexture: float }),
      );
    }
  }

  return out;
}
