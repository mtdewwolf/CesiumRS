// Particles and emitters, point cloud shading, imagery feature info and
// configuration, the empty-tile discard policy and voxel keyframe nodes,
// evaluated by the pinned CesiumJS engine.
//
// Emitter cases seed CesiumMath's Mersenne Twister, so the Rust port must
// draw the same random numbers in the same order.
import { suite } from "./lib.mjs";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;
const core = async (name) => (await import(`@cesium/engine/Source/Core/${name}.js`)).default;

export default async function sceneParticlesImagery() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  const Cartesian2 = await core("Cartesian2");
  const Cartesian3 = await core("Cartesian3");
  const Color = await core("Color");
  const CesiumMath = await core("Math");
  const Particle = await scene("Particle");
  const ParticleEmitter = await scene("ParticleEmitter");
  const emitters = {
    box: await scene("BoxEmitter"),
    circle: await scene("CircleEmitter"),
    cone: await scene("ConeEmitter"),
    sphere: await scene("SphereEmitter"),
  };
  const PointCloudShading = await scene("PointCloudShading");
  const ImageryLayerFeatureInfo = await scene("ImageryLayerFeatureInfo");
  const KeyframeNode = await scene("KeyframeNode");
  const ImageryConfiguration = (await import("@cesium/engine/Source/Scene/Model/ImageryConfiguration.js")).default;

  const c3 = (c) => [c.x, c.y, c.z];
  const color = (c) => [c.red, c.green, c.blue, c.alpha];
  const particleState = (p) => ({
    mass: p.mass, position: c3(p.position), velocity: c3(p.velocity), life: p.life, startColor: color(p.startColor), endColor: color(p.endColor),
    startScale: p.startScale, endScale: p.endScale, imageSize: [p.imageSize.x, p.imageSize.y], age: p.age, normalizedAge: p.normalizedAge,
  });
  const particleOptions = (o) => ({
    ...o,
    position: o.position && Cartesian3.fromArray(o.position),
    velocity: o.velocity && Cartesian3.fromArray(o.velocity),
    startColor: o.startColor && Color.unpack(o.startColor),
    endColor: o.endColor && Color.unpack(o.endColor),
    imageSize: o.imageSize && Cartesian2.fromArray(o.imageSize),
  });

  // ---- Particle ----
  {
    const s = add("Particle");
    const options = [
      {},
      { mass: 2, position: [1, 2, 3], velocity: [0.5, -1, 0], life: 3, startColor: [1, 0, 0, 1], endColor: [0, 0, 1, 0.5], startScale: 2, endScale: 0.5, imageSize: [16, 8] },
      { life: 0 },
    ];
    for (const o of options) s.add("new", [o], (opts) => particleState(new Particle(particleOptions(opts))));
    // Updates, with an optional update function: "gravity" pulls velocity down.
    const scripts = [
      [{ velocity: [1, 0, 0], life: 1 }, [0.25, 0.25, 0.5, 0.25], null],
      [{ velocity: [0, 0, 10] }, [0.1, 1, 100], "gravity"],
      [{ position: [1, 1, 1], velocity: [-1, 2, 0.5], life: 2.5 }, [1, 1, 0.5, 0.0001], "drag"],
    ];
    for (const [o, dts, fn] of scripts) {
      s.add("update", [o, dts, fn], (opts, steps, name) => {
        const p = new Particle(particleOptions(opts));
        const update =
          name === "gravity" ? (q, dt) => { q.velocity.z -= 9.8 * dt; }
          : name === "drag" ? (q, dt) => { Cartesian3.multiplyByScalar(q.velocity, 1 - 0.5 * dt, q.velocity); q.mass += dt; }
          : undefined;
        return steps.map((dt) => [p.update(dt, update), particleState(p)]);
      });
    }
  }

  // ---- emitters ----
  const make = (kind, arg) => (arg === null ? new emitters[kind]() : new emitters[kind](kind === "box" ? Cartesian3.fromArray(arg) : arg));
  const emitMany = (emitter, seed, count) => {
    CesiumMath.setRandomNumberSeed(seed);
    const p = new Particle();
    const result = [];
    for (let i = 0; i < count; i++) {
      emitter.emit(p);
      result.push([c3(p.position), c3(p.velocity)]);
    }
    return result;
  };
  const emitterCases = {
    box: [null, [2, 4, 6], [0, 1, 1]],
    circle: [null, 3.5, 1e-3],
    cone: [null, 0.1, CesiumMath.toRadians(80)],
    sphere: [null, 10, 0.25],
  };
  for (const [kind, args] of Object.entries(emitterCases)) {
    const name = { box: "BoxEmitter", circle: "CircleEmitter", cone: "ConeEmitter", sphere: "SphereEmitter" }[kind];
    const s = add(name);
    const property = kind === "box" ? "dimensions" : kind === "cone" ? "angle" : "radius";
    const read = (e) => (kind === "box" ? c3(e.dimensions) : e[property]);
    for (const [i, arg] of args.entries()) {
      s.add("emit", [arg, 1234 + i, 25], (a, seed, count) => {
        const e = make(kind, a);
        return [read(e), emitMany(e, seed, count)];
      });
    }
    const setTo = kind === "box" ? [3, 0.5, 2] : kind === "cone" ? 0.7 : 4;
    s.add("set", [setTo, 99, 10], (value, seed, count) => {
      const e = make(kind, null);
      e[property] = kind === "box" ? Cartesian3.fromArray(value) : value;
      return [read(e), emitMany(e, seed, count)];
    });
  }

  // ---- ParticleEmitter ----
  {
    const s = add("ParticleEmitter");
    s.add("new", [], () => new ParticleEmitter());
    // The interface contract: emit sets only position and velocity, in place.
    for (const kind of Object.keys(emitters)) {
      s.add("emit", [kind, 7], (k, seed) => {
        CesiumMath.setRandomNumberSeed(seed);
        const p = new Particle(particleOptions({ mass: 3, life: 9, startScale: 2, imageSize: [4, 4] }));
        const position = p.position;
        const velocity = p.velocity;
        make(k, null).emit(p);
        return [p.position === position, p.velocity === velocity, particleState(p)];
      });
    }
  }

  // ---- PointCloudShading ----
  {
    const s = add("PointCloudShading");
    const options = [
      null,
      { attenuation: true, geometricErrorScale: 2, maximumAttenuation: 4, baseResolution: 0.5, eyeDomeLighting: false, eyeDomeLightingStrength: 0.25,
        eyeDomeLightingRadius: 3, backFaceCulling: true, normalShading: false },
      { maximumAttenuation: 0, eyeDomeLightingStrength: 0 },
    ];
    const fields = (p) => [p.attenuation, p.geometricErrorScale, p.maximumAttenuation ?? null, p.baseResolution ?? null, p.eyeDomeLighting,
      p.eyeDomeLightingStrength, p.eyeDomeLightingRadius, p.backFaceCulling, p.normalShading];
    for (const o of options) s.add("new", [o], (opts) => fields(new PointCloudShading(opts ?? undefined)));
    for (const [drawBuffers, fragmentDepth] of [[true, true], [true, false], [false, true], [false, false]]) {
      s.add("isSupported", [drawBuffers, fragmentDepth], (d, f) => PointCloudShading.isSupported({ context: { drawBuffers: d, fragmentDepth: f } }));
    }
  }

  // ---- ImageryLayerFeatureInfo ----
  {
    const s = add("ImageryLayerFeatureInfo");
    s.add("new", [], () => {
      const info = new ImageryLayerFeatureInfo();
      return [info.name ?? null, info.description ?? null, info.position ?? null, info.data ?? null, info.imageryLayer ?? null];
    });
    const names = [
      { name: "a", title: "b" },
      { title: "b", NAME: "c" },
      { FeatureName: "x", Title: "t" },
      { subtitle: "s", nickname: "n" },
      // A name match outranks a later title match.
      { nickname: "n", subtitle: "s", streetName: "later" },
      { name: "", title: 0, other: 1, nameless: false, label_title: null },
      { Name: "first", name: "second" },
      { a_title: "t1", b_title: "t2" },
      { name: { x: 1 } },
      { 10: "ten", name2: "n" },
      { NAMÉ: 1, "İTitle": "dotted" },
      { TITLE_NAME: "both", title: "plain" },
      "name",
      ["first", "second"],
      {},
    ];
    for (const p of names) {
      s.add("name", [p], (props) => {
        const info = new ImageryLayerFeatureInfo();
        info.configureNameFromProperties(props);
        return info.name ?? null;
      });
    }
    const descriptions = [
      { name: "Road", lanes: 4, toll: false, width: 12.5, big: 1e21, small: 1e-7 },
      { nested: { a: 1, b: { c: "deep" } }, list: [1, "two", null, { three: 3 }], skipped: null },
      { html: "<b>bold</b> & <i>x</i>", 7: "seven", 2: "two" },
      {},
      "ab",
      [true, 0],
    ];
    for (const p of descriptions) {
      s.add("description", [p], (props) => {
        const info = new ImageryLayerFeatureInfo();
        info.configureDescriptionFromProperties(props);
        return info.description;
      });
    }
  }

  // ---- KeyframeNode ----
  {
    const s = add("KeyframeNode");
    const node = (k) => [k.spatialNode ?? null, k.keyframe, k.state, k.content ?? null, k.megatextureIndex, k.priority, k.highPriorityFrameNumber];
    s.add("loadState", [], () => ({ ...KeyframeNode.LoadState }));
    s.add("lifecycle", [{ level: 2, x: 1 }, 3], (spatial, keyframe) => {
      const k = new KeyframeNode(spatial, keyframe);
      const created = node(k);
      k.state = KeyframeNode.LoadState.LOADED;
      k.content = { destroy: () => undefined };
      k.megatextureIndex = 5;
      k.priority = 12.5;
      k.highPriorityFrameNumber = 40;
      const loaded = node({ ...k, content: k.content ? "content" : null });
      k.unload();
      return [created, loaded, node(k)];
    });
    const pairs = [[1, 2, 0, 4], [-Number.MAX_VALUE, 0, 7, 7], [Infinity, -Infinity, 9, 1], [0.1, 0.3, 2, 10]];
    for (const [pa, pb, ka, kb] of pairs) {
      s.add("compare", [pa, pb, ka, kb], (a, b, c, d) => {
        const x = new KeyframeNode({}, c);
        const y = new KeyframeNode({}, d);
        x.priority = a;
        y.priority = b;
        return [KeyframeNode.priorityComparator(x, y), KeyframeNode.priorityComparator(y, x), KeyframeNode.searchComparator(x, y)];
      });
    }
  }

  // ---- ImageryConfiguration ----
  {
    const s = add("ImageryConfiguration");
    const layers = [
      { show: true, alpha: 1, brightness: 1, contrast: 1, hue: 0, saturation: 1, gamma: 1, colorToAlpha: null },
      { show: false, alpha: 0.25, brightness: 2, contrast: 0.5, hue: 3.1, saturation: 0, gamma: 2.2, colorToAlpha: [1, 0, 0.5, 1] },
    ];
    for (const layer of layers) {
      s.add("new", [layer], (l) => {
        const c = new ImageryConfiguration({ ...l, colorToAlpha: l.colorToAlpha ? Color.unpack(l.colorToAlpha) : undefined });
        return [c.show, c.alpha, c.brightness, c.contrast, c.hue, c.saturation, c.gamma, c.colorToAlpha ? color(c.colorToAlpha) : null];
      });
    }
  }

  // ---- DiscardEmptyTileImagePolicy ----
  {
    // Node has no Image; the policy only needs an object with a src.
    globalThis.Image ??= class Image {};
    const DiscardEmptyTileImagePolicy = await scene("DiscardEmptyTileImagePolicy");
    const s = add("DiscardEmptyTileImagePolicy");
    s.add("policy", [], () => {
      const a = new DiscardEmptyTileImagePolicy();
      const b = new DiscardEmptyTileImagePolicy({});
      const empty = DiscardEmptyTileImagePolicy.EMPTY_IMAGE;
      return {
        src: empty.src,
        shared: empty === DiscardEmptyTileImagePolicy.EMPTY_IMAGE,
        ready: [a.isReady(), b.isReady()],
        discardEmpty: [a.shouldDiscardImage(empty), b.shouldDiscardImage(empty)],
        discardOther: a.shouldDiscardImage(new Image()),
        discardCopy: a.shouldDiscardImage(Object.assign(new Image(), { src: empty.src })),
      };
    });
  }
  return out;
}
