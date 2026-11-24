// SwiftShader WebGL2 reference captures from pinned @cesium/engine 23.0.0
// for Cluster 2 Scene Globe & Terrain Rendering modules:
// - Scene/Globe
// - Scene/GlobeSurfaceShaderSet
// - Scene/GlobeSurfaceTileProvider
// - Scene/GlobeTranslucencyState
// - Scene/TerrainFillMesh
//
// Usage: CHROMIUM_PATH=/path/to/chrome node tools/cesium/generate-scene-globe-terrain.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const outDir = join(here, '../../tests/data');
const outPath = join(outDir, 'scene-globe-terrain.json');
const pkg = JSON.parse(readFileSync(join(here, 'node_modules/@cesium/engine/package.json'), 'utf8'));

const modules = {
  Scene: [
    'Globe',
    'GlobeSurfaceShaderSet',
    'GlobeSurfaceTile',
    'GlobeSurfaceTileProvider',
    'GlobeTranslucency',
    'GlobeTranslucencyState',
    'TerrainFillMesh',
    'QuadtreePrimitive',
    'QuadtreeTile',
    'QuadtreeOccluders',
    'QuadtreeTileLoadState',
    'TerrainState',
    'TileSelectionResult',
    'TileBoundingRegion',
    'SceneMode',
    'ShadowMode',
    'SplitDirection',
    'Fog',
    'Camera',
    'FrameState',
    'CreditDisplay',
    'ClippingPlane',
    'ClippingPlaneCollection',
    'ClippingPolygon',
    'ClippingPolygonCollection',
    'ImageryLayer',
    'ImageryLayerCollection',
    'ImageryState',
    'Material',
    'BlendingState',
    'CullFace',
    'DepthFunction',
  ],
  Renderer: [
    'Context',
    'ContextLimits',
    'DrawCommand',
    'Pass',
    'RenderState',
    'ShaderProgram',
    'ShaderSource',
    'Texture',
    'PixelDatatype',
  ],
  Core: [
    'BoundingSphere',
    'Cartesian2',
    'Cartesian3',
    'Cartesian4',
    'Cartographic',
    'Color',
    'Credit',
    'CullingVolume',
    'Ellipsoid',
    'EllipsoidTerrainProvider',
    'Event',
    'GeographicProjection',
    'GeographicTilingScheme',
    'HeightmapTerrainData',
    'Intersect',
    'JulianDate',
    'Math',
    'Matrix4',
    'NearFarScalar',
    'OrthographicFrustum',
    'OrthographicOffCenterFrustum',
    'PerspectiveFrustum',
    'PixelFormat',
    'Plane',
    'Ray',
    'Rectangle',
    'TerrainEncoding',
    'TerrainMesh',
    'TerrainQuantization',
    'Visibility',
    'WebMercatorProjection',
    'WebMercatorTilingScheme',
  ],
  Shaders: [
    'AtmosphereCommon',
    'GroundAtmosphere',
    'GlobeVS',
    'GlobeFS',
  ],
};

const contents = Object.entries(modules)
  .flatMap(([dir, names]) =>
    names.map((n) => {
      const exportName = n === 'Math' ? 'CesiumMath' : n.split('/').pop();
      return `export {default as ${exportName}} from '@cesium/engine/Source/${dir}/${n}.js';`;
    }),
  )
  .join('\n');

const bundle = await build({
  stdin: { contents, resolveDir: here },
  bundle: true,
  write: false,
  format: 'esm',
  platform: 'browser',
});

const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') {
    res.setHeader('Content-Type', 'text/javascript');
    res.end(bundle.outputFiles[0].contents);
    return;
  }
  res.end('<!doctype html><title>Scene Globe & Terrain Validation</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

let browser;
try {
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH,
    headless: true,
    args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);

  const captured = await page.evaluate(async () => {
    let phase;
    try {
    const ref = await import('/reference.mjs');
    // Run Cesium’s heightmap mesh calculation directly in the fixture page.
    ref.HeightmapTerrainData.prototype.createMesh = async function (options) {
      return this._createMeshSync(options);
    };
    const makeCanvas = (w, h) => {
      const c = document.createElement('canvas');
      c.width = w;
      c.height = h;
      return c;
    };
    const attrs = { alpha: true, depth: true, stencil: true, antialias: false, preserveDrawingBuffer: true };
    const vec3 = (v) => (v ? [v.x, v.y, v.z] : null);
    const vec4 = (v) => (v ? [v.x, v.y, v.z, v.w] : null);
    const col4 = (c) => (c ? [c.red, c.green, c.blue, c.alpha] : null);
    const nfs = (s) => (s ? [s.near, s.nearValue, s.far, s.farValue] : null);
    const rect4 = (r) => (r ? [r.west, r.south, r.east, r.north] : null);
    const modeName = (m) =>
      m === ref.SceneMode.SCENE3D ? 'SCENE3D'
      : m === ref.SceneMode.SCENE2D ? 'SCENE2D'
      : m === ref.SceneMode.COLUMBUS_VIEW ? 'COLUMBUS_VIEW'
      : 'MORPHING';
    const visName = (v) =>
      v === ref.Visibility.FULL ? 'FULL'
      : v === ref.Visibility.PARTIAL ? 'PARTIAL'
      : 'NONE';

    const makeMockCreditDisplay = () => ({
      credits: [],
      addCreditToNextFrame(c) { this.credits.push(c); },
    });

    const makeSceneAndFrame = (context, opts = {}) => {
      const mode = opts.mode ?? ref.SceneMode.SCENE3D;
      const projection = opts.projection ?? new ref.GeographicProjection(opts.ellipsoid ?? ref.Ellipsoid.WGS84);
      const mockScene = {
        drawingBufferWidth: 64,
        drawingBufferHeight: 64,
        mapProjection: projection,
        mode,
        cameraUnderground: opts.cameraUnderground ?? false,
        context,
        globe: opts.globe,
      };
      const camera = new ref.Camera(mockScene);
      if (mode === ref.SceneMode.SCENE3D) {
        camera.position = opts.cameraPosition ?? new ref.Cartesian3(0.0, -25000000.0, 0.0);
        camera.direction = opts.cameraDirection ?? ref.Cartesian3.clone(ref.Cartesian3.UNIT_Y);
        camera.up = opts.cameraUp ?? ref.Cartesian3.clone(ref.Cartesian3.UNIT_Z);
        camera.right = opts.cameraRight ?? ref.Cartesian3.clone(ref.Cartesian3.UNIT_X);
        camera.frustum = opts.frustum ?? new ref.PerspectiveFrustum({
          fov: ref.CesiumMath.toRadians(60.0),
          aspectRatio: 1.0,
          near: 1.0,
          far: 100000000.0,
        });
      } else if (mode === ref.SceneMode.SCENE2D) {
        camera.position = opts.cameraPosition ?? new ref.Cartesian3(0.0, 0.0, 20000000.0);
        camera.direction = new ref.Cartesian3(0.0, 0.0, -1.0);
        camera.up = new ref.Cartesian3(0.0, 1.0, 0.0);
        camera.right = new ref.Cartesian3(1.0, 0.0, 0.0);
        camera.frustum = new ref.OrthographicOffCenterFrustum({
          left: -20000000.0,
          right: 20000000.0,
          bottom: -20000000.0,
          top: 20000000.0,
          near: 1.0,
          far: 100000000.0,
        });
      } else {
        camera.position = opts.cameraPosition ?? new ref.Cartesian3(0.0, 0.0, 20000000.0);
        camera.direction = new ref.Cartesian3(0.0, 0.0, -1.0);
        camera.up = new ref.Cartesian3(0.0, 1.0, 0.0);
        camera.right = new ref.Cartesian3(1.0, 0.0, 0.0);
        camera.frustum = new ref.PerspectiveFrustum({
          fov: ref.CesiumMath.toRadians(60.0),
          aspectRatio: 1.0,
          near: 1.0,
          far: 100000000.0,
        });
      }
      camera.update(mode);
      const frameState = new ref.FrameState(context, makeMockCreditDisplay(), {
        preUpdate: { raiseEvent() {} },
        postUpdate: { raiseEvent() {} },
      });
      frameState.mode = mode;
      frameState.mapProjection = projection;
      frameState.camera = camera;
      frameState.cullingVolume = camera.frustum.computeCullingVolume(
        camera.positionWC,
        camera.directionWC,
        camera.upWC,
      );
      frameState.passes.render = opts.passRender ?? true;
      frameState.passes.pick = opts.passPick ?? false;
      frameState.passes.depth = opts.passDepth ?? false;
      frameState.cameraUnderground = opts.cameraUnderground ?? false;
      frameState.fog.enabled = opts.fogEnabled ?? false;
      frameState.fog.density = opts.fogDensity ?? 0.0002;
      frameState.fog.renderable = opts.fogRenderable ?? true;
      frameState.fog.minimumBrightness = opts.fogMinBrightness ?? 0.03;
      frameState.verticalExaggeration = opts.exaggeration ?? 1.0;
      frameState.verticalExaggerationRelativeHeight = opts.exaggerationRelativeHeight ?? 0.0;
      frameState.time = new ref.JulianDate(2451545, 0);
      frameState.shadowState.shadowsEnabled = opts.shadowsEnabled ?? false;
      frameState.frameNumber = opts.frameNumber ?? 1;
      frameState.globeTranslucencyState = new ref.GlobeTranslucencyState();
      context.uniformState.update(frameState);
      return { scene: mockScene, camera, frameState };
    };

    // =========================================================================
    // 1. Scene/Globe
    // =========================================================================
    phase = "globeSection";
    const globeSection = await (async () => {
      const canvas = makeCanvas(64, 64);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;

      const snapshotDefaults = (g) => ({
        ellipsoidRadii: vec3(g.ellipsoid.radii),
        show: g.show,
        maximumScreenSpaceError: g.maximumScreenSpaceError,
        tileCacheSize: g.tileCacheSize,
        loadingDescendantLimit: g.loadingDescendantLimit,
        preloadAncestors: g.preloadAncestors,
        preloadSiblings: g.preloadSiblings,
        depthTestAgainstTerrain: g.depthTestAgainstTerrain,
        enableLighting: g.enableLighting,
        lambertDiffuseMultiplier: g.lambertDiffuseMultiplier,
        dynamicAtmosphereLighting: g.dynamicAtmosphereLighting,
        dynamicAtmosphereLightingFromSun: g.dynamicAtmosphereLightingFromSun,
        showGroundAtmosphere: g.showGroundAtmosphere,
        atmosphereLightIntensity: g.atmosphereLightIntensity,
        atmosphereRayleighCoefficient: vec3(g.atmosphereRayleighCoefficient),
        atmosphereMieCoefficient: vec3(g.atmosphereMieCoefficient),
        atmosphereRayleighScaleHeight: g.atmosphereRayleighScaleHeight,
        atmosphereMieScaleHeight: g.atmosphereMieScaleHeight,
        atmosphereMieAnisotropy: g.atmosphereMieAnisotropy,
        lightingFadeOutDistance: g.lightingFadeOutDistance,
        lightingFadeInDistance: g.lightingFadeInDistance,
        nightFadeOutDistance: g.nightFadeOutDistance,
        nightFadeInDistance: g.nightFadeInDistance,
        showWaterEffect: g.showWaterEffect,
        shadows: g.shadows,
        atmosphereHueShift: g.atmosphereHueShift,
        atmosphereSaturationShift: g.atmosphereSaturationShift,
        atmosphereBrightnessShift: g.atmosphereBrightnessShift,
        showSkirts: g.showSkirts,
        backFaceCulling: g.backFaceCulling,
        vertexShadowDarkness: g.vertexShadowDarkness,
        baseColor: col4(g.baseColor),
        undergroundColor: col4(g.undergroundColor),
        undergroundColorAlphaByDistance: nfs(g.undergroundColorAlphaByDistance),
        cartographicLimitRectangle: rect4(g.cartographicLimitRectangle),
        oceanNormalMapUrlEndsWith: g.oceanNormalMapUrl.endsWith('Assets/Textures/waterNormalsSmall.jpg'),
        fillHighlightColorDefined: g.fillHighlightColor !== undefined,
        translucencyEnabled: g.translucency.enabled,
        tilesLoaded: g.tilesLoaded,
      });

      const defaultWgs84 = new ref.Globe();
      const defaultUnitSphere = new ref.Globe(ref.Ellipsoid.UNIT_SPHERE);
      const defaults = {
        wgs84: snapshotDefaults(defaultWgs84),
        unitSphere: snapshotDefaults(defaultUnitSphere),
      };
      defaultUnitSphere.destroy();

      // Frame lifecycle, tileLoadProgressEvent, getHeight, pickWorldCoordinates, pick (3D/2D/CV), terrainProvider replacement, destroy
      const globe = defaultWgs84;
      const progressEvents = [];
      globe.tileLoadProgressEvent.addEventListener((n) => progressEvents.push(n));

      const heightBeforeLoad = globe.getHeight(new ref.Cartographic(0, 0, 0)) ?? null;
      const tilesLoadedBefore = globe.tilesLoaded;

      const { scene, frameState } = makeSceneAndFrame(context, {
        mode: ref.SceneMode.SCENE3D,
        globe,
      });

      const runFrame = (fs) => {
        fs.frameNumber += 1;
        fs.commandList.length = 0;
        fs.globeTranslucencyState.update(scene);
        context.uniformState.update(fs);
        globe.update(fs);
        globe.beginFrame(fs);
        globe.render(fs);
        globe.endFrame(fs);
      };

      for (let i = 0; i < 6; i++) {
        runFrame(frameState);
        await new Promise(requestAnimationFrame);
      }

      const renderedTiles3D = Array.from(globe._surface._tilesRenderedThisFrame).map((t) => ({
        level: t.level,
        x: t.x,
        y: t.y,
        rectangle: rect4(t.rectangle),
      }));

      const commands3D = frameState.commandList.map((c) => ({
        pass: c.pass,
        receiveShadows: c.receiveShadows,
        castShadows: c.castShadows,
        count: c.count,
      }));

      // getHeight queries across the globe
      const heightQueries = [
        [0.0, 0.0],
        [0.4, 0.2],
        [-1.2, -0.5],
        [-2.0, 0.6],
      ].map(([lon, lat]) => ({
        longitude: lon,
        latitude: lat,
        height: globe.getHeight(new ref.Cartographic(lon, lat, 0.0)) ?? null,
      }));

      // 3D pick tests: rays pointing toward points on rendered surface tiles + miss ray
      const make3DRay = (lon, lat) => {
        const pt = globe.ellipsoid.cartographicToCartesian(new ref.Cartographic(lon, lat, 0.0));
        const origin = ref.Cartesian3.multiplyByScalar(pt, 2.0, new ref.Cartesian3());
        const direction = ref.Cartesian3.normalize(ref.Cartesian3.negate(pt, new ref.Cartesian3()), new ref.Cartesian3());
        return { origin, direction, ray: new ref.Ray(origin, direction) };
      };
      const pick3DCases = [
        { name: 'southern_hemi_front', lon: -1.5, lat: -0.2 },
        { name: 'northern_hemi_front', lon: -1.2, lat: 0.3 },
        { name: 'near_equator_front', lon: -1.57, lat: 0.05 },
      ].map((c) => {
        const { origin, direction, ray } = make3DRay(c.lon, c.lat);
        const worldHit = globe.pickWorldCoordinates(ray, scene, true);
        const pickHit = globe.pick(ray, scene);
        return {
          name: c.name,
          origin: vec3(origin),
          direction: vec3(direction),
          worldHit: vec3(worldHit),
          pickHit: vec3(pickHit),
        };
      });
      // Miss ray
      {
        const origin = new ref.Cartesian3(0.0, -25000000.0, 0.0);
        const direction = new ref.Cartesian3(1.0, 0.0, 0.0);
        const ray = new ref.Ray(origin, direction);
        pick3DCases.push({
          name: 'miss_tangent',
          origin: vec3(origin),
          direction: vec3(direction),
          worldHit: vec3(globe.pickWorldCoordinates(ray, scene, true)),
          pickHit: vec3(globe.pick(ray, scene)),
        });
      }

      // 2D and Columbus View picking
      const pickProjected = (mode) => {
        const projSetup = makeSceneAndFrame(context, { mode, globe, frameNumber: frameState.frameNumber + 1 });
        projSetup.scene.globe = globe;
        for (let i = 0; i < 4; i++) {
          projSetup.frameState.frameNumber += 1;
          projSetup.frameState.commandList.length = 0;
          projSetup.frameState.globeTranslucencyState.update(projSetup.scene);
          context.uniformState.update(projSetup.frameState);
          globe.update(projSetup.frameState);
          globe.beginFrame(projSetup.frameState);
          globe.render(projSetup.frameState);
          globe.endFrame(projSetup.frameState);
        }
        // Projected coordinates: Cesium 2D/CV world space is (x = height, y = projected.x, z = projected.y)
        // Pick a cartographic point (lon=0.5, lat=0.25)
        const targetCarto = new ref.Cartographic(0.5, 0.25, 0.0);
        const projected = projSetup.scene.mapProjection.project(targetCarto);
        const origin = new ref.Cartesian3(1000000.0, projected.x, projected.y);
        const direction = new ref.Cartesian3(-1.0, 0.0, 0.0);
        const ray = new ref.Ray(origin, direction);
        const worldHit = globe.pickWorldCoordinates(ray, projSetup.scene, true);
        const pickHit = globe.pick(ray, projSetup.scene);
        return {
          mode: modeName(mode),
          origin: vec3(origin),
          direction: vec3(direction),
          worldHit: vec3(worldHit),
          pickHit: vec3(pickHit),
          renderedTileCount: globe._surface._tilesRenderedThisFrame.size,
        };
      };
      const pick2D = pickProjected(ref.SceneMode.SCENE2D);
      const pickCV = pickProjected(ref.SceneMode.COLUMBUS_VIEW);

      // show = false produces zero commands
      globe.show = false;
      runFrame(frameState);
      const commandsWhenHidden = frameState.commandList.length;
      globe.show = true;

      // terrainProviderChanged event
      let providerChanges = 0;
      globe.terrainProviderChanged.addEventListener(() => { providerChanges++; });
      const sameProvider = globe.terrainProvider;
      globe.terrainProvider = sameProvider;
      const changesAfterSame = providerChanges;
      const replacement = new ref.EllipsoidTerrainProvider({ ellipsoid: ref.Ellipsoid.WGS84 });
      globe.terrainProvider = replacement;
      const changesAfterReplacement = providerChanges;

      const isDestroyedBefore = globe.isDestroyed();
      globe.destroy();
      const isDestroyedAfter = globe.isDestroyed();
      const glError = gl.getError();
      context.destroy();

      return {
        defaults,
        heightBeforeLoad,
        tilesLoadedBefore,
        tilesLoadedAfter: true,
        progressEventsHadNonZero: progressEvents.some((n) => n > 0),
        progressEventsLast: progressEvents[progressEvents.length - 1] ?? null,
        renderedTiles3D,
        commands3DCount: commands3D.length,
        allCommandsPassGlobe: commands3D.every((c) => c.pass === ref.Pass.GLOBE),
        allCommandsReceiveShadows: commands3D.every((c) => c.receiveShadows),
        anyCommandCastShadows: commands3D.some((c) => c.castShadows),
        heightQueries,
        pick3DCases,
        pick2D,
        pickCV,
        commandsWhenHidden,
        changesAfterSame,
        changesAfterReplacement,
        isDestroyedBefore,
        isDestroyedAfter,
        glError,
      };
    })();

    // =========================================================================
    // 2. Scene/GlobeSurfaceShaderSet
    // =========================================================================
    phase = "shaderSetSection";
    const shaderSetSection = (() => {
      const canvas = makeCanvas(32, 32);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;

      const shaderSet = new ref.GlobeSurfaceShaderSet();
      const setBaseShaders = (material) => {
        const defines = [];
        const fragmentSources = [ref.AtmosphereCommon, ref.GroundAtmosphere];
        if (material) {
          fragmentSources.push(material.shaderSource);
          defines.push('APPLY_MATERIAL');
        }
        fragmentSources.push(ref.GlobeFS);
        shaderSet.baseVertexShaderSource = new ref.ShaderSource({
          sources: [ref.AtmosphereCommon, ref.GroundAtmosphere, ref.GlobeVS],
          defines: defines.slice(),
        });
        shaderSet.baseFragmentShaderSource = new ref.ShaderSource({
          sources: fragmentSources,
          defines: defines.slice(),
        });
        shaderSet.material = material;
      };
      setBaseShaders(undefined);

      const makeEncoding = (quantized, hasVertexNormals, hasGeodetic) => {
        if (quantized) {
          const min = new ref.Cartesian3(-1000, -1000, -1000);
          const max = new ref.Cartesian3(1000, 1000, 1000);
          const aabb = { minimum: min, maximum: max };
          return new ref.TerrainEncoding(
            new ref.Cartesian3(6378137, 0, 0),
            aabb,
            0.0,
            100.0,
            ref.Matrix4.IDENTITY,
            hasVertexNormals,
            true,
            hasGeodetic,
            hasGeodetic ? 2.0 : 1.0,
            0.0,
          );
        }
        return new ref.TerrainEncoding(
          new ref.Cartesian3(6378137, 0, 0),
          undefined,
          undefined,
          undefined,
          undefined,
          hasVertexNormals,
          true,
          hasGeodetic,
          hasGeodetic ? 2.0 : 1.0,
          0.0,
        );
      };

      const clippingPlanes = new ref.ClippingPlaneCollection({
        planes: [
          new ref.ClippingPlane(new ref.Cartesian3(1, 0, 0), 100.0),
          new ref.ClippingPlane(new ref.Cartesian3(0, 1, 0), -50.0),
        ],
        unionClippingRegions: true,
        enabled: true,
      });
      clippingPlanes.update({ context });

      const clippingPolygons = new ref.ClippingPolygonCollection({
        polygons: [
          new ref.ClippingPolygon({
            positions: ref.Cartesian3.fromDegreesArray([-10, -10, 10, -10, 10, 10, -10, 10]),
          }),
        ],
        inverse: true,
        enabled: true,
      });
      clippingPolygons.update({ context });

      const specs = [
        {
          name: 'base_3d_0tex',
          mode: ref.SceneMode.SCENE3D,
          numberOfDayTextures: 0,
        },
        {
          name: 'scene2d_mercator_1tex_quantized',
          mode: ref.SceneMode.SCENE2D,
          numberOfDayTextures: 1,
          quantized: true,
          useWebMercatorProjection: true,
          applyAlpha: true,
          applyDayNightAlpha: true,
          applyBrightness: true,
        },
        {
          name: 'columbus_2tex_adjustments',
          mode: ref.SceneMode.COLUMBUS_VIEW,
          numberOfDayTextures: 2,
          applyContrast: true,
          applyHue: true,
          applySaturation: true,
          applyGamma: true,
          applySplit: true,
          hasImageryLayerCutout: true,
          colorToAlpha: true,
          clippedByBoundaries: true,
        },
        {
          name: 'morphing_lighting_fog_atmosphere',
          mode: ref.SceneMode.MORPHING,
          numberOfDayTextures: 1,
          enableLighting: true,
          hasVertexNormals: true,
          dynamicAtmosphereLighting: true,
          dynamicAtmosphereLightingFromSun: true,
          showGroundAtmosphere: true,
          perFragmentGroundAtmosphere: true,
          enableFog: true,
          colorCorrect: true,
          hasGeodeticSurfaceNormals: true,
          hasExaggeration: true,
          showUndergroundColor: true,
          translucent: true,
          highlightFillTile: true,
        },
        {
          name: 'lighting_daynight_water_waves',
          mode: ref.SceneMode.SCENE3D,
          numberOfDayTextures: 0,
          enableLighting: true,
          hasVertexNormals: false,
          hasWaterMask: true,
          showReflectiveOcean: true,
          showOceanWaves: true,
        },
        {
          name: 'clipping_planes_1tex',
          mode: ref.SceneMode.SCENE3D,
          numberOfDayTextures: 1,
          enableClippingPlanes: true,
          clippingPlanes,
        },
        {
          name: 'clipping_polygons_1tex',
          mode: ref.SceneMode.SCENE3D,
          numberOfDayTextures: 1,
          enableClippingPolygons: true,
          clippingPolygons,
        },
      ];

      const runSpec = (spec) => {
        const encoding = makeEncoding(!!spec.quantized, !!spec.hasVertexNormals, !!spec.hasGeodeticSurfaceNormals);
        const surfaceTile = { renderedMesh: { encoding }, surfaceShader: undefined };
        const opts = {
          frameState: { mode: spec.mode, context },
          surfaceTile,
          numberOfDayTextures: spec.numberOfDayTextures,
          applyBrightness: !!spec.applyBrightness,
          applyContrast: !!spec.applyContrast,
          applyHue: !!spec.applyHue,
          applySaturation: !!spec.applySaturation,
          applyGamma: !!spec.applyGamma,
          applyAlpha: !!spec.applyAlpha,
          applyDayNightAlpha: !!spec.applyDayNightAlpha,
          applySplit: !!spec.applySplit,
          hasWaterMask: !!spec.hasWaterMask,
          showReflectiveOcean: !!spec.showReflectiveOcean,
          showOceanWaves: !!spec.showOceanWaves,
          enableLighting: !!spec.enableLighting,
          dynamicAtmosphereLighting: !!spec.dynamicAtmosphereLighting,
          dynamicAtmosphereLightingFromSun: !!spec.dynamicAtmosphereLightingFromSun,
          showGroundAtmosphere: !!spec.showGroundAtmosphere,
          perFragmentGroundAtmosphere: !!spec.perFragmentGroundAtmosphere,
          hasVertexNormals: !!spec.hasVertexNormals,
          useWebMercatorProjection: !!spec.useWebMercatorProjection,
          enableFog: !!spec.enableFog,
          enableClippingPlanes: !!spec.enableClippingPlanes,
          clippingPlanes: spec.clippingPlanes,
          enableClippingPolygons: !!spec.enableClippingPolygons,
          clippingPolygons: spec.clippingPolygons,
          clippedByBoundaries: !!spec.clippedByBoundaries,
          hasImageryLayerCutout: !!spec.hasImageryLayerCutout,
          colorCorrect: !!spec.colorCorrect,
          highlightFillTile: !!spec.highlightFillTile,
          colorToAlpha: !!spec.colorToAlpha,
          hasGeodeticSurfaceNormals: !!spec.hasGeodeticSurfaceNormals,
          hasExaggeration: !!spec.hasExaggeration,
          showUndergroundColor: !!spec.showUndergroundColor,
          translucent: !!spec.translucent,
        };
        const prog1 = shaderSet.getShaderProgram(opts);
        // Trigger compilation on SwiftShader to validate GLSL is valid WebGL2!
        prog1._bind();
        const prog2 = shaderSet.getShaderProgram(opts);
        // Also test tile-level miss -> shaderSet-level hit with a second tile
        const surfaceTile2 = { renderedMesh: { encoding }, surfaceShader: undefined };
        const prog3 = shaderSet.getShaderProgram({ ...opts, surfaceTile: surfaceTile2 });

        const vsDefines = prog1.vertexShaderSource.defines.filter((d) => d.length > 0);
        const fsDefines = prog1.fragmentShaderSource.defines.filter((d) => d.length > 0);
        const vsSources = prog1.vertexShaderSource.sources;
        const fsSources = prog1.fragmentShaderSource.sources;
        return {
          name: spec.name,
          mode: modeName(spec.mode),
          numberOfDayTextures: spec.numberOfDayTextures,
          flags: surfaceTile.surfaceShader.flags,
          cacheHitSameTile: prog1 === prog2,
          cacheHitSecondTile: prog1 === prog3,
          programId: prog1.id,
          vsDefines,
          fsDefines,
          positionModeSource: vsSources[vsSources.length - 2],
          yFractionSource: vsSources[vsSources.length - 1],
          computeDayColorSource: fsSources[fsSources.length - 1],
          hasClippingPlanesGlsl: fsSources[0].includes('getClippingPlane'),
          hasClippingPolygonsVs: vsSources[0].includes('unpackClippingExtents'),
          hasClippingPolygonsFs: fsSources[0].includes('clipPolygons'),
          glError: gl.getError(),
        };
      };

      const cases = specs.map(runSpec);

      // Material invalidation check
      const colorMaterial = ref.Material.fromType('Color');
      setBaseShaders(colorMaterial);
      const materialCase = runSpec({
        name: 'material_color_1tex',
        mode: ref.SceneMode.SCENE3D,
        numberOfDayTextures: 1,
        applyAlpha: true,
      });
      const baseCaseId = cases.find((c) => c.name === 'base_3d_0tex').programId;
      const materialChangedProgram = materialCase.programId !== baseCaseId;

      clippingPlanes.destroy();
      clippingPolygons.destroy();
      shaderSet.destroy();
      const destroyed = shaderSet.isDestroyed();
      const glError = gl.getError();
      context.destroy();

      return {
        cases,
        materialCase,
        materialChangedProgram,
        destroyed,
        glError,
      };
    })();

    // =========================================================================
    // 3. Scene/GlobeTranslucencyState
    // =========================================================================
    phase = "translucencySection";
    const translucencySection = (() => {
      const canvas = makeCanvas(16, 16);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;

      const derivedTypeNames = [
        'OpaqueFront',
        'OpaqueBack',
        'DepthFront',
        'DepthBack',
        'DepthBoth',
        'TranslucentFront',
        'TranslucentBack',
        'ManualDepthFront',
        'ManualDepthBack',
        'PickFront',
        'PickBack',
      ];
      const derivedPropNames = [
        'opaqueFrontFaceCommand',
        'opaqueBackFaceCommand',
        'depthOnlyFrontFaceCommand',
        'depthOnlyBackFaceCommand',
        'depthOnlyFrontAndBackFaceCommand',
        'translucentFrontFaceCommand',
        'translucentBackFaceCommand',
        'translucentFrontFaceManualDepthTestCommand',
        'translucentBackFaceManualDepthTestCommand',
        'pickFrontFaceCommand',
        'pickBackFaceCommand',
      ];

      const makeDummyGlobe = (cfg) => {
        const tr = new ref.GlobeTranslucency();
        tr.enabled = cfg.enabled ?? false;
        tr.frontFaceAlpha = cfg.frontFaceAlpha ?? 1.0;
        if (cfg.frontFaceAlphaByDistance) {
          tr.frontFaceAlphaByDistance = new ref.NearFarScalar(...cfg.frontFaceAlphaByDistance);
        }
        tr.backFaceAlpha = cfg.backFaceAlpha ?? 1.0;
        if (cfg.backFaceAlphaByDistance) {
          tr.backFaceAlphaByDistance = new ref.NearFarScalar(...cfg.backFaceAlphaByDistance);
        }
        if (cfg.rectangleDeg) {
          tr.rectangle = ref.Rectangle.fromDegrees(...cfg.rectangleDeg);
        }
        return {
          show: cfg.show ?? true,
          baseColor: new ref.Color(0.0, 0.0, 0.5, cfg.baseAlpha ?? 1.0),
          depthTestAgainstTerrain: cfg.depthTestAgainstTerrain ?? false,
          translucency: tr,
        };
      };

      const stateConfigs = [
        { name: 'disabled', enabled: false, show: true },
        { name: 'globe_hidden', enabled: true, frontFaceAlpha: 0.5, show: false },
        { name: 'enabled_opaque_1', enabled: true, frontFaceAlpha: 1.0, backFaceAlpha: 1.0, baseAlpha: 1.0 },
        { name: 'base_color_translucent', enabled: true, frontFaceAlpha: 1.0, backFaceAlpha: 1.0, baseAlpha: 0.5 },
        {
          name: 'front_only_manual_depth_dateline',
          enabled: true,
          frontFaceAlpha: 0.5,
          frontFaceAlphaByDistance: [10.0, 0.2, 200.0, 0.8],
          backFaceAlpha: 1.0,
          depthTestAgainstTerrain: false,
          depthTexture: true,
          mode: ref.SceneMode.SCENE3D,
          cameraUnderground: false,
          rectangleDeg: [175.0, -30.0, -175.0, 30.0],
        },
        {
          name: 'front_only_depth_test_against_terrain',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 1.0,
          depthTestAgainstTerrain: true,
          depthTexture: true,
          mode: ref.SceneMode.SCENE3D,
          cameraUnderground: false,
        },
        {
          name: 'front_only_no_depth_texture',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 1.0,
          depthTestAgainstTerrain: false,
          depthTexture: false,
          mode: ref.SceneMode.SCENE3D,
          cameraUnderground: false,
        },
        {
          name: 'front_only_underground',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 1.0,
          depthTestAgainstTerrain: false,
          depthTexture: true,
          mode: ref.SceneMode.SCENE3D,
          cameraUnderground: true,
        },
        {
          name: 'both_faces_above_ground',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 0.4,
          backFaceAlphaByDistance: [50.0, 0.5, 500.0, 1.0],
          depthTestAgainstTerrain: false,
          depthTexture: true,
          mode: ref.SceneMode.SCENE3D,
          cameraUnderground: false,
        },
        {
          name: 'both_faces_underground',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 0.4,
          depthTestAgainstTerrain: false,
          depthTexture: true,
          mode: ref.SceneMode.SCENE3D,
          cameraUnderground: true,
        },
        {
          name: 'scene_2d_translucent',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 1.0,
          depthTestAgainstTerrain: false,
          depthTexture: true,
          mode: ref.SceneMode.SCENE2D,
          cameraUnderground: false,
        },
        {
          name: 'columbus_view_front_only',
          enabled: true,
          frontFaceAlpha: 0.5,
          backFaceAlpha: 1.0,
          depthTestAgainstTerrain: false,
          depthTexture: true,
          mode: ref.SceneMode.COLUMBUS_VIEW,
          cameraUnderground: false,
        },
      ];

      const stateCases = stateConfigs.map((cfg) => {
        const state = new ref.GlobeTranslucencyState();
        const globe = makeDummyGlobe(cfg);
        const scene = {
          globe,
          cameraUnderground: cfg.cameraUnderground ?? false,
          mode: cfg.mode ?? ref.SceneMode.SCENE3D,
          context: { depthTexture: cfg.depthTexture ?? true },
        };
        state.update(scene);

        const listTypes = (arr, len) => Array.from({ length: len }, (_, i) => derivedTypeNames[arr[i]]);
        return {
          name: cfg.name,
          input: {
            enabled: cfg.enabled ?? false,
            show: cfg.show ?? true,
            baseAlpha: cfg.baseAlpha ?? 1.0,
            frontFaceAlpha: cfg.frontFaceAlpha ?? 1.0,
            frontFaceAlphaByDistance: cfg.frontFaceAlphaByDistance ?? null,
            backFaceAlpha: cfg.backFaceAlpha ?? 1.0,
            backFaceAlphaByDistance: cfg.backFaceAlphaByDistance ?? null,
            depthTestAgainstTerrain: cfg.depthTestAgainstTerrain ?? false,
            depthTexture: cfg.depthTexture ?? true,
            mode: modeName(cfg.mode ?? ref.SceneMode.SCENE3D),
            cameraUnderground: cfg.cameraUnderground ?? false,
            rectangleDeg: cfg.rectangleDeg ?? null,
          },
          frontFaceAlphaByDistance: nfs(state.frontFaceAlphaByDistance),
          backFaceAlphaByDistance: nfs(state.backFaceAlphaByDistance),
          translucent: state.translucent,
          sunVisibleThroughGlobe: state.sunVisibleThroughGlobe,
          environmentVisible: state.environmentVisible,
          useDepthPlane: state.useDepthPlane,
          numberOfTextureUniforms: state.numberOfTextureUniforms,
          rectangle: rect4(state.rectangle),
          renderCommands: listTypes(state._derivedCommandTypes, state._derivedCommandsLength),
          blendCommands: listTypes(state._derivedBlendCommandTypes, state._derivedBlendCommandsLength),
          pickCommands: listTypes(state._derivedPickCommandTypes, state._derivedPickCommandsLength),
        };
      });

      // Inspect derived commands (render states, passes, shader defines/sources) for all 11 derived command types!
      const baseVs = new ref.ShaderSource({
        defines: ['TRANSLUCENT', 'GROUND_ATMOSPHERE', 'FOG'],
        sources: ['in vec3 a_pos; out vec3 v_positionEC; void main() { v_positionEC = a_pos; gl_Position = vec4(a_pos, 1.0); }'],
      });
      const baseFs = new ref.ShaderSource({
        defines: ['TRANSLUCENT', 'GROUND_ATMOSPHERE', 'FOG'],
        sources: ['in vec3 v_positionEC; void main() { out_FragColor = vec4(0.2, 0.4, 0.6, 0.5); }'],
      });
      const baseProgram = ref.ShaderProgram.fromCache({
        context,
        vertexShaderSource: baseVs,
        fragmentShaderSource: baseFs,
        attributeLocations: { a_pos: 0 },
      });
      const clippedFs = new ref.ShaderSource({
        defines: ['TRANSLUCENT', 'GROUND_ATMOSPHERE', 'FOG', 'ENABLE_CLIPPING_PLANES'],
        sources: ['in vec3 v_positionEC; void main() { if (v_positionEC.x < 0.0) discard; out_FragColor = vec4(0.2, 0.4, 0.6, 0.5); }'],
      });
      const clippedProgram = ref.ShaderProgram.fromCache({
        context,
        vertexShaderSource: baseVs,
        fragmentShaderSource: clippedFs,
        attributeLocations: { a_pos: 0 },
      });

      const baseRs = ref.RenderState.fromCache({
        cull: { enabled: false },
        depthTest: { enabled: true, func: ref.DepthFunction.LESS },
      });

      const collectDerived = (program, cfgList) => {
        const out = {};
        for (const cfg of cfgList) {
          const state = new ref.GlobeTranslucencyState();
          const globe = makeDummyGlobe(cfg);
          const scene = {
            globe,
            cameraUnderground: cfg.cameraUnderground ?? false,
            mode: cfg.mode ?? ref.SceneMode.SCENE3D,
            context,
          };
          state.update(scene);
          const cmd = new ref.DrawCommand({
            shaderProgram: program,
            renderState: baseRs,
            uniformMap: {},
            pass: ref.Pass.GLOBE,
          });
          state.updateDerivedCommands(cmd, { context, frameNumber: 1 });
          const derivedObj = cmd.derivedCommands.globeTranslucency;
          for (let i = 0; i < derivedPropNames.length; i++) {
            const prop = derivedPropNames[i];
            const dcmd = derivedObj[prop];
            if (dcmd && !out[derivedTypeNames[i]]) {
              const rs = dcmd.renderState;
              const fsJoined = dcmd.shaderProgram.fragmentShaderSource.sources.join('\n');
              out[derivedTypeNames[i]] = {
                pass: dcmd.pass,
                pickOnly: dcmd.pickOnly,
                cullEnabled: rs.cull.enabled,
                cullFace: rs.cull.face,
                colorMask: [rs.colorMask.red, rs.colorMask.green, rs.colorMask.blue, rs.colorMask.alpha],
                depthMask: rs.depthMask,
                blendingEnabled: rs.blending.enabled,
                vsDefines: dcmd.shaderProgram.vertexShaderSource.defines.slice(),
                fsDefines: dcmd.shaderProgram.fragmentShaderSource.defines.slice(),
                hasReplacedMain: fsJoined.includes('czm_globe_translucency_main'),
                hasClassificationUniform: fsJoined.includes('u_classificationTexture'),
                hasManualDepthCheck: fsJoined.includes('czm_globeDepthTexture'),
                isMinimalDepthOnly: fsJoined.trim() === 'void main() \n{ \n    out_FragColor = vec4(1.0); \n}',
              };
            }
          }
        }
        return out;
      };

      const allDerivedConfigs = [
        // Front only above ground + manual depth -> DepthFront, OpaqueBack, ManualDepthFront, PickFront
        { enabled: true, frontFaceAlpha: 0.5, backFaceAlpha: 1.0, depthTestAgainstTerrain: false, cameraUnderground: false },
        // Front only underground + manual depth -> DepthBack, OpaqueFront, ManualDepthBack, PickBack
        { enabled: true, frontFaceAlpha: 0.5, backFaceAlpha: 1.0, depthTestAgainstTerrain: false, cameraUnderground: true },
        // Both faces above ground -> DepthBoth, TranslucentBack, TranslucentFront, PickBack, PickFront
        { enabled: true, frontFaceAlpha: 0.5, backFaceAlpha: 0.5, depthTestAgainstTerrain: false, cameraUnderground: false },
      ];

      const derivedCommands = collectDerived(baseProgram, allDerivedConfigs);
      const clippedDepthOnly = collectDerived(clippedProgram, [allDerivedConfigs[0]]).DepthFront;

      const glError = gl.getError();
      context.destroy();
      return {
        stateCases,
        derivedCommands,
        clippedDepthOnly,
        glError,
      };
    })();

    // =========================================================================
    // 4. Scene/GlobeSurfaceTileProvider
    // =========================================================================
    phase = "tileProviderSection";
    const tileProviderSection = await (async () => {
      const canvas = makeCanvas(64, 64);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;

      // 4a. Defaults
      const defaultTerrain = new ref.EllipsoidTerrainProvider();
      const defaultShaderSet = new ref.GlobeSurfaceShaderSet();
      const defaultImagery = new ref.ImageryLayerCollection();
      const defaultProvider = new ref.GlobeSurfaceTileProvider({
        terrainProvider: defaultTerrain,
        imageryLayers: defaultImagery,
        surfaceShaderSet: defaultShaderSet,
      });
      const defaults = {
        baseColor: col4(defaultProvider.baseColor),
        lightingFadeOutDistance: defaultProvider.lightingFadeOutDistance,
        lightingFadeInDistance: defaultProvider.lightingFadeInDistance,
        shadows: defaultProvider.shadows,
        showSkirts: defaultProvider.showSkirts,
        backFaceCulling: defaultProvider.backFaceCulling,
        vertexShadowDarkness: defaultProvider.vertexShadowDarkness,
        cartographicLimitRectangle: rect4(defaultProvider.cartographicLimitRectangle),
        lambertDiffuseMultiplier: defaultProvider.lambertDiffuseMultiplier,
        zoomedOutOceanSpecularIntensity: defaultProvider.zoomedOutOceanSpecularIntensity,
      };
      defaultProvider.destroy();
      defaultShaderSet.destroy();

      // 4b. Visibility, distance, priority, canRefine
      const globe = new ref.Globe();
      const { scene, frameState } = makeSceneAndFrame(context, {
        mode: ref.SceneMode.SCENE3D,
        globe,
      });
      const provider = globe._surface.tileProvider;

      // Load root tiles
      for (let i = 0; i < 6; i++) {
        frameState.frameNumber += 1;
        frameState.commandList.length = 0;
        frameState.globeTranslucencyState.update(scene);
        context.uniformState.update(frameState);
        globe.update(frameState);
        globe.beginFrame(frameState);
        globe.render(frameState);
        globe.endFrame(frameState);
        await new Promise(requestAnimationFrame);
      }

      const rootTiles = globe._surface._levelZeroTiles;
      const root0 = rootTiles[0]; // west hemisphere (-180..0)
      const root1 = rootTiles[1]; // east hemisphere (0..180)
      // Ensure both root tiles have real terrain meshes & vertexArrays loaded
      for (const r of [root0, root1]) {
        for (let step = 0; step < 6 && !r.data?.vertexArray; step++) {
          provider.loadTile(frameState, r);
          await new Promise(requestAnimationFrame);
        }
      }
      const occluders = globe._surface._occluders;

      phase = `tileProviderSection visibility (root meshes: ${!!root0.data?.vertexArray}, ${!!root1.data?.vertexArray})`;
      const distRoot0 = provider.computeDistanceToTile(root0, frameState);
      const distRoot1 = provider.computeDistanceToTile(root1, frameState);
      const visRoot0 = visName(provider.computeTileVisibility(root0, frameState, occluders));
      const visRoot1 = visName(provider.computeTileVisibility(root1, frameState, occluders));
      const priorityRoot0 = provider.computeTileLoadPriority(root0, frameState);
      const priorityRoot1 = provider.computeTileLoadPriority(root1, frameState);
      const canRefineRoot0 = provider.canRefine(root0);

      // Unloaded child conservative distance (inherits parent min/max)
      const swChild = root0.southwestChild;
      const distChildInherited = provider.computeDistanceToTile(swChild, frameState);

      // Cartographic limit rectangle excluding root0 (west hemisphere)
      provider.cartographicLimitRectangle = ref.Rectangle.fromDegrees(40.0, -20.0, 70.0, 20.0);
      const visRoot0ExcludedByLimit = visName(provider.computeTileVisibility(root0, frameState, occluders));
      const visRoot1PartialLimit = visName(provider.computeTileVisibility(root1, frameState, occluders));
      const root1ClippedByBoundaries = root1.data.clippedByBoundaries;
      provider.cartographicLimitRectangle = ref.Rectangle.clone(ref.Rectangle.MAX_VALUE);

      // Dense fog culling (above ground vs underground)
      frameState.fog.enabled = true;
      frameState.fog.density = 1.0;
      const visRoot0DenseFogAbove = visName(provider.computeTileVisibility(root0, frameState, occluders));
      frameState.cameraUnderground = true;
      const visRoot0DenseFogUnderground = visName(provider.computeTileVisibility(root0, frameState, occluders));
      frameState.cameraUnderground = false;
      frameState.fog.enabled = false;
      frameState.fog.density = 0.0002;

      // Clipping plane culling root0
      const cullPlane = new ref.ClippingPlaneCollection({
        planes: [new ref.ClippingPlane(new ref.Cartesian3(1, 0, 0), -10000000.0)],
        enabled: true,
      });
      cullPlane.update(frameState);
      provider.clippingPlanes = cullPlane;
      const visRoot0ClippedOut = visName(provider.computeTileVisibility(root0, frameState, occluders));
      provider.clippingPlanes = undefined;

      // 4c. Multipass imagery batching, render states, shader defines, and uniform values
      // Build 3 ready imagery layers with distinct adjustments and a real WebGL2 texture
      const dummyTex = new ref.Texture({
        context,
        width: 2,
        height: 2,
        source: { width: 2, height: 2, arrayBufferView: new Uint8Array(16).fill(180) },
      });
      const makeMockImageryLayer = (idx, opts) => {
        const layer = new ref.ImageryLayer({
          ready: true,
          rectangle: ref.Rectangle.MAX_VALUE,
          tilingScheme: new ref.GeographicTilingScheme(),
          tileWidth: 16,
          tileHeight: 16,
          maximumLevel: 0,
          errorEvent: new ref.Event(),
          requestImage: () => undefined,
        }, opts);
        layer._layerIndex = idx;
        return layer;
      };
      const layer0 = makeMockImageryLayer(0, {
        alpha: 0.85,
        dayAlpha: 0.9,
        nightAlpha: 0.7,
        brightness: 1.25,
        contrast: 1.1,
        hue: 0.2,
        saturation: 1.3,
        gamma: 2.0,
        splitDirection: ref.SplitDirection.LEFT,
        cutoutRectangle: ref.Rectangle.fromDegrees(-90.0, -30.0, -45.0, 30.0),
        colorToAlpha: new ref.Color(1.0, 0.0, 0.0, 1.0),
        colorToAlphaThreshold: 0.05,
      });
      const layer1 = makeMockImageryLayer(1, {
        brightness: 1.0,
        alpha: 1.0,
      });
      const layer2 = makeMockImageryLayer(2, {
        brightness: 1.5,
        contrast: 0.8,
        alpha: 0.6,
      });
      const attachLayerToTile = (tile, layer, useWebMercatorT) => {
        const img = {
          imageryLayer: layer,
          x: tile.x,
          y: tile.y,
          level: tile.level,
          rectangle: tile.rectangle,
          state: ref.ImageryState.READY,
          texture: dummyTex,
          textureWebMercator: dummyTex,
          credits: undefined,
        };
        const ti = {
          readyImagery: img,
          loadingImagery: undefined,
          textureCoordinateRectangle: new ref.Cartesian4(0.0, 0.0, 1.0, 1.0),
          textureTranslationAndScale: new ref.Cartesian4(0.0, 0.0, 1.0, 1.0),
          useWebMercatorT,
          freeResources() {},
        };
        tile.data.imagery.push(ti);
      };

      root0.data.imagery.length = 0;
      attachLayerToTile(root0, layer0, false);
      attachLayerToTile(root0, layer1, false);
      attachLayerToTile(root0, layer2, false);

      // Force texture unit cap to 3 with shadowsEnabled = true -> maxTextures = 2 -> 2 batches (2 + 1)
      const origMaxUnits = ref.ContextLimits._maximumTextureImageUnits;
      ref.ContextLimits._maximumTextureImageUnits = 3;
      frameState.shadowState.shadowsEnabled = true;
      frameState.commandList.length = 0;
      frameState.globeTranslucencyState.update(scene);

      provider.beginUpdate(frameState);
      provider.showTileThisFrame(root0, frameState);
      phase += " multipass";
      provider.endUpdate(frameState);

      const snapshotCommand = (cmd) => {
        const u = cmd.uniformMap;
        const p = u.properties;
        const rs = cmd.renderState;
        return {
          pass: cmd.pass,
          count: cmd.count,
          castShadows: cmd.castShadows,
          receiveShadows: cmd.receiveShadows,
          cullEnabled: rs.cull.enabled,
          depthTestEnabled: rs.depthTest.enabled,
          depthFunc: rs.depthTest.func,
          blendingEnabled: rs.blending.enabled,
          vsDefines: cmd.shaderProgram.vertexShaderSource.defines.filter((d) => d.length > 0),
          fsDefines: cmd.shaderProgram.fragmentShaderSource.defines.filter((d) => d.length > 0),
          uniforms: {
            initialColor: vec4(u.u_initialColor()),
            zoomedOutOceanSpecularIntensity: u.u_zoomedOutOceanSpecularIntensity(),
            atmosphereLightIntensity: u.u_atmosphereLightIntensity(),
            atmosphereRayleighCoefficient: vec3(u.u_atmosphereRayleighCoefficient()),
            atmosphereMieCoefficient: vec3(u.u_atmosphereMieCoefficient()),
            atmosphereRayleighScaleHeight: u.u_atmosphereRayleighScaleHeight(),
            atmosphereMieScaleHeight: u.u_atmosphereMieScaleHeight(),
            atmosphereMieAnisotropy: u.u_atmosphereMieAnisotropy(),
            lightingFadeDistance: [p.lightingFadeDistance.x, p.lightingFadeDistance.y],
            nightFadeDistance: [p.nightFadeDistance.x, p.nightFadeDistance.y],
            center3D: vec3(u.u_center3D()),
            verticalExaggerationAndRelativeHeight: [p.verticalExaggerationAndRelativeHeight.x, p.verticalExaggerationAndRelativeHeight.y],
            tileRectangle: vec4(u.u_tileRectangle()),
            southAndNorthLatitude: [p.southAndNorthLatitude.x, p.southAndNorthLatitude.y],
            southMercatorYAndOneOverHeight: [p.southMercatorYAndOneOverHeight.x, p.southMercatorYAndOneOverHeight.y],
            minMaxHeight: [p.minMaxHeight.x, p.minMaxHeight.y],
            cartographicLimitRectangle: vec4(u.u_cartographicLimitRectangle()),
            translucencyRectangle: vec4(u.u_translucencyRectangle()),
            frontFaceAlphaByDistance: vec4(u.u_frontFaceAlphaByDistance()),
            backFaceAlphaByDistance: vec4(u.u_backFaceAlphaByDistance()),
            undergroundColor: col4(u.u_undergroundColor()),
            undergroundColorAlphaByDistance: vec4(u.u_undergroundColorAlphaByDistance()),
            lambertDiffuseMultiplier: u.u_lambertDiffuseMultiplier(),
            vertexShadowDarkness: u.u_vertexShadowDarkness(),
            hsbShift: vec3(u.u_hsbShift()),
            fillHighlightColor: col4(u.u_fillHighlightColor()),
            dayTextureCount: p.dayTextures.length,
            dayTextureAlpha: p.dayTextureAlpha.slice(0, p.dayTextures.length),
            dayTextureDayAlpha: p.dayTextureDayAlpha.slice(0, p.dayTextures.length),
            dayTextureNightAlpha: p.dayTextureNightAlpha.slice(0, p.dayTextures.length),
            dayTextureBrightness: p.dayTextureBrightness.slice(0, p.dayTextures.length),
            dayTextureContrast: p.dayTextureContrast.slice(0, p.dayTextures.length),
            dayTextureHue: p.dayTextureHue.slice(0, p.dayTextures.length),
            dayTextureSaturation: p.dayTextureSaturation.slice(0, p.dayTextures.length),
            dayTextureOneOverGamma: p.dayTextureOneOverGamma.slice(0, p.dayTextures.length),
            dayTextureSplit: p.dayTextureSplit.slice(0, p.dayTextures.length),
            dayTextureUseWebMercatorT: p.dayTextureUseWebMercatorT.slice(0, p.dayTextures.length),
            dayTextureTranslationAndScale: p.dayTextureTranslationAndScale.slice(0, p.dayTextures.length).map(vec4),
            dayTextureTexCoordsRectangle: p.dayTextureTexCoordsRectangle.slice(0, p.dayTextures.length).map(vec4),
            dayTextureCutoutRectangles: p.dayTextureCutoutRectangles.slice(0, p.dayTextures.length).map(vec4),
            colorsToAlpha: p.colorsToAlpha.slice(0, p.dayTextures.length).map(vec4),
          },
        };
      };

      const multipassBatches = frameState.commandList.map(snapshotCommand);
      ref.ContextLimits._maximumTextureImageUnits = origMaxUnits;
      frameState.shadowState.shadowsEnabled = false;

      // 4d. Underground swap of frontFaceAlphaByDistance / backFaceAlphaByDistance + showSkirts false
      root0.data.imagery.length = 0;
      globe.translucency.enabled = true;
      globe.translucency.frontFaceAlpha = 0.25;
      globe.translucency.frontFaceAlphaByDistance = new ref.NearFarScalar(100.0, 0.4, 2000.0, 0.8);
      globe.translucency.backFaceAlpha = 0.75;
      globe.translucency.backFaceAlphaByDistance = new ref.NearFarScalar(50.0, 0.2, 1000.0, 0.6);

      const captureAlphaSwap = (underground) => {
        scene.cameraUnderground = underground;
        frameState.cameraUnderground = underground;
        frameState.commandList.length = 0;
        frameState.globeTranslucencyState.update(scene);
        provider.beginUpdate(frameState);
        provider.showTileThisFrame(root0, frameState);
        provider.endUpdate(frameState);
        const cmd = frameState.commandList[0];
        return {
          underground,
          commandCount: frameState.commandList.length,
          indexCount: cmd.count,
          indexCountWithoutSkirts: root0.data.renderedMesh.indexCountWithoutSkirts,
          indexCountWithSkirts: root0.data.renderedMesh.indices.length,
          frontFaceAlphaByDistance: vec4(cmd.uniformMap.u_frontFaceAlphaByDistance()),
          backFaceAlphaByDistance: vec4(cmd.uniformMap.u_backFaceAlphaByDistance()),
        };
      };
      phase = "tileProviderSection alpha swap";
      const translucencyAboveGround = captureAlphaSwap(false);
      const translucencyUnderground = captureAlphaSwap(true);
      globe.translucency.enabled = false;
      scene.cameraUnderground = false;
      frameState.cameraUnderground = false;

      // 4e. WebMercator 2D uniform calculation (u_tileRectangle, u_southAndNorthLatitude, u_southMercatorYAndOneOverHeight)
      const mercSetup = makeSceneAndFrame(context, {
        mode: ref.SceneMode.SCENE2D,
        projection: new ref.WebMercatorProjection(ref.Ellipsoid.WGS84),
        globe,
      });
      mercSetup.frameState.commandList.length = 0;
      mercSetup.frameState.globeTranslucencyState.update(mercSetup.scene);
      provider.beginUpdate(mercSetup.frameState);
      provider.showTileThisFrame(root0, mercSetup.frameState);
      phase = "tileProviderSection mercator";
      provider.endUpdate(mercSetup.frameState);
      const mercCmd = mercSetup.frameState.commandList[0];
      const webMercator2DUniforms = {
        tileRectangle: vec4(mercCmd.uniformMap.u_tileRectangle()),
        southAndNorthLatitude: [mercCmd.uniformMap.properties.southAndNorthLatitude.x, mercCmd.uniformMap.properties.southAndNorthLatitude.y],
        southMercatorYAndOneOverHeight: [mercCmd.uniformMap.properties.southMercatorYAndOneOverHeight.x, mercCmd.uniformMap.properties.southMercatorYAndOneOverHeight.y],
        zoomedOutOceanSpecularIntensity: mercCmd.uniformMap.u_zoomedOutOceanSpecularIntensity(),
        vsUsesMercator: mercCmd.shaderProgram.vertexShaderSource.sources.join('\n').includes('get2DMercatorYPositionFraction'),
      };

      // 4f. Fill tile handling + fillHighlightColor alpha > 0 vs alpha == 0 + enableLighting on fill tile (terrainProvider.hasVertexNormals == false)
      const fillTile = root0.southwestChild.northeastChild.southwestChild.northeastChild;
      fillTile.data = new ref.GlobeSurfaceTile();
      fillTile.data.tileBoundingRegion = new ref.TileBoundingRegion({
        rectangle: fillTile.rectangle,
        minimumHeight: 0.0,
        maximumHeight: 0.0,
      });
      provider.enableLighting = true;

      const captureFillHighlight = (color) => {
        provider.fillHighlightColor = color;
        frameState.commandList.length = 0;
        frameState.globeTranslucencyState.update(scene);
        provider.beginUpdate(frameState);
        provider.showTileThisFrame(fillTile, frameState);
        provider.endUpdate(frameState);
        const cmd = frameState.commandList[0];
        const fsDefines = cmd.shaderProgram.fragmentShaderSource.defines.filter((d) => d.length > 0);
        return {
          hasHighlightFillDefine: fsDefines.includes('HIGHLIGHT_FILL_TILE'),
          hasDayNightShadingDefine: fsDefines.includes('ENABLE_DAYNIGHT_SHADING'),
          hasVertexLightingDefine: fsDefines.includes('ENABLE_VERTEX_LIGHTING'),
          fillMeshDefined: fillTile.data.fill !== undefined && fillTile.data.fill.mesh !== undefined,
        };
      };
      phase = "tileProviderSection fill highlight";
      const fillHighlightOpaque = captureFillHighlight(new ref.Color(1.0, 0.0, 0.0, 0.5));
      const fillHighlightZeroAlpha = captureFillHighlight(new ref.Color(1.0, 0.0, 0.0, 0.0));

      dummyTex.destroy();
      globe.destroy();
      const glError = gl.getError();
      context.destroy();

      return {
        defaults,
        visibilityAndDistance: {
          distRoot0,
          distRoot1,
          visRoot0,
          visRoot1,
          priorityRoot0,
          priorityRoot1,
          canRefineRoot0,
          distChildInherited,
          visRoot0ExcludedByLimit,
          visRoot1PartialLimit,
          root1ClippedByBoundaries,
          visRoot0DenseFogAbove,
          visRoot0DenseFogUnderground,
          visRoot0ClippedOut,
        },
        multipassBatches,
        translucencyAboveGround,
        translucencyUnderground,
        webMercator2DUniforms,
        fillHighlightOpaque,
        fillHighlightZeroAlpha,
        glError,
      };
    })();

    // =========================================================================
    // 5. Scene/TerrainFillMesh
    // =========================================================================
    phase = "fillMeshSection";
    const fillMeshSection = (() => {
      const canvas = makeCanvas(32, 32);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;

      const ellipsoid = ref.Ellipsoid.WGS84;
      const scheme = new ref.GeographicTilingScheme({ ellipsoid });
      const terrainProvider = new ref.EllipsoidTerrainProvider({ ellipsoid, tilingScheme: scheme });
      const imageryLayers = new ref.ImageryLayerCollection();
      const shaderSet = new ref.GlobeSurfaceShaderSet();
      const tileProvider = new ref.GlobeSurfaceTileProvider({
        terrainProvider,
        imageryLayers,
        surfaceShaderSet: shaderSet,
      });
      const quadtree = new ref.QuadtreePrimitive({ tileProvider });
      const { frameState } = makeSceneAndFrame(context, { mode: ref.SceneMode.SCENE3D });

      // Helper to create a real HeightmapTerrainData mesh for a tile
      const makeHeightmapTile = (tile, width, height, heightFn, exaggeration = 1.0) => {
        const buf = new Float32Array(width * height);
        for (let r = 0; r < height; r++) {
          const v = 1.0 - r / (height - 1);
          for (let c = 0; c < width; c++) {
            const u = c / (width - 1);
            buf[r * width + c] = heightFn(u, v);
          }
        }
        const td = new ref.HeightmapTerrainData({
          buffer: buf,
          width,
          height,
          structure: { heightScale: 1.0, heightOffset: 0.0, elementsPerHeight: 1, stride: 1, elementMultiplier: 256.0, isBigEndian: false },
        });
        const mesh = td._createMeshSync({
          tilingScheme: scheme,
          x: tile.x,
          y: tile.y,
          level: tile.level,
          exaggeration,
          exaggerationRelativeHeight: 0.0,
        });
        tile.data = new ref.GlobeSurfaceTile();
        tile.data.mesh = mesh;
        tile.data.vertexArray = ref.GlobeSurfaceTile._createVertexArrayForMesh(context, mesh);
        tile.data.terrainState = ref.TerrainState.READY;
        tile.data.tileBoundingRegion = new ref.TileBoundingRegion({
          rectangle: tile.rectangle,
          ellipsoid,
          minimumHeight: mesh.minimumHeight,
          maximumHeight: mesh.maximumHeight,
        });
        return mesh;
      };

      const makeFillTile = (tile, minH = 0.0, maxH = 0.0) => {
        tile.data = new ref.GlobeSurfaceTile();
        tile.data.tileBoundingRegion = new ref.TileBoundingRegion({
          rectangle: tile.rectangle,
          ellipsoid,
          minimumHeight: minH,
          maximumHeight: maxH,
        });
        return tile;
      };

      const decodeEdge = (mesh, indices) => {
        const enc = mesh.encoding;
        const verts = mesh.vertices;
        return indices.map((idx) => {
          const uv = enc.decodeTextureCoordinates(verts, idx, new ref.Cartesian2());
          const pos = enc.decodePosition(verts, idx, new ref.Cartesian3());
          const h = enc.decodeHeight(verts, idx);
          return { index: idx, uv: [uv.x, uv.y], height: h, position: vec3(pos) };
        });
      };

      const snapshotFill = (tile) => {
        const mesh = tile.data.fill.mesh;
        const enc = mesh.encoding;
        const verts = mesh.vertices;
        const vertexCount = verts.length / enc.stride;
        const decodedVertices = [];
        for (let i = 0; i < vertexCount; i++) {
          const uv = enc.decodeTextureCoordinates(verts, i, new ref.Cartesian2());
          const pos = enc.decodePosition(verts, i, new ref.Cartesian3());
          const h = enc.decodeHeight(verts, i);
          decodedVertices.push({ uv: [uv.x, uv.y], height: h, position: vec3(pos) });
        }
        return {
          level: tile.level,
          x: tile.x,
          y: tile.y,
          rectangle: rect4(tile.rectangle),
          geometricError: tileProvider.getLevelMaximumGeometricError(tile.level),
          vertexCount,
          vertexCountWithoutSkirts: mesh.vertexCountWithoutSkirts,
          indexCount: mesh.indices.length,
          indexCountWithoutSkirts: mesh.indexCountWithoutSkirts,
          indices: Array.from(mesh.indices),
          minimumHeight: mesh.minimumHeight,
          maximumHeight: mesh.maximumHeight,
          stride: enc.stride,
          hasVertexNormals: enc.hasVertexNormals,
          hasWebMercatorT: enc.hasWebMercatorT,
          hasGeodeticSurfaceNormals: enc.hasGeodeticSurfaceNormals,
          westIndices: Array.from(mesh.westIndicesSouthToNorth),
          southIndices: Array.from(mesh.southIndicesEastToWest),
          eastIndices: Array.from(mesh.eastIndicesNorthToSouth),
          northIndices: Array.from(mesh.northIndicesWestToEast),
          westEdge: decodeEdge(mesh, mesh.westIndicesSouthToNorth),
          southEdge: decodeEdge(mesh, mesh.southIndicesEastToWest),
          eastEdge: decodeEdge(mesh, mesh.eastIndicesNorthToSouth),
          northEdge: decodeEdge(mesh, mesh.northIndicesWestToEast),
          vertices: decodedVertices,
        };
      };

      // Build quadtree level zero tiles
      const levelZero = ref.QuadtreeTile.createLevelZeroTiles(scheme);
      quadtree._levelZeroTiles = levelZero;

      // Helper to navigate down to a specific (level, x, y)
      const getTile = (level, x, y) => {
        let cur = levelZero[x >> level];
        for (let l = 1; l <= level; l++) {
          const shift = level - l;
          const bx = (x >> shift) & 1;
          const by = (y >> shift) & 1;
          if (bx === 0 && by === 0) cur = cur.northwestChild;
          else if (bx === 1 && by === 0) cur = cur.northeastChild;
          else if (bx === 0 && by === 1) cur = cur.southwestChild;
          else cur = cur.southeastChild;
        }
        return cur;
      };

      // Case 1: Low-detail grid fill (level 1 tile with realistic heightmap geometric error, constant height 150 -> 9x9 grid = 81 verts)
      const lowDetailTile = getTile(1, 0, 0);
      makeFillTile(lowDetailTile, 150.0, 150.0);
      lowDetailTile.data.fill = new ref.TerrainFillMesh(lowDetailTile);
      const origGetError = tileProvider.getLevelMaximumGeometricError.bind(tileProvider);
      tileProvider.getLevelMaximumGeometricError = (lvl) => 38000.0 / (1 << lvl);
      lowDetailTile.data.fill.update(tileProvider, frameState);
      const lowDetailGrid = {
        level: lowDetailTile.level,
        rectangle: rect4(lowDetailTile.rectangle),
        geometricError: tileProvider.getLevelMaximumGeometricError(lowDetailTile.level),
        vertexCountWithoutSkirts: lowDetailTile.data.fill.mesh.vertexCountWithoutSkirts,
        minimumHeight: lowDetailTile.data.fill.mesh.minimumHeight,
        maximumHeight: lowDetailTile.data.fill.mesh.maximumHeight,
        centerHeight: lowDetailTile.data.fill.mesh.encoding.decodeHeight(lowDetailTile.data.fill.mesh.vertices, 40),
      };
      tileProvider.getLevelMaximumGeometricError = origGetError;

      // Case 2: High-detail isolated fill (level 8, no neighbors, min=100, max=300 -> height 200, 5 verts, 12 indices)
      const isolatedTile = getTile(8, 100, 50);
      makeFillTile(isolatedTile, 100.0, 300.0);
      isolatedTile.data.fill = new ref.TerrainFillMesh(isolatedTile);
      isolatedTile.data.fill.update(tileProvider, frameState);
      const highDetailIsolated = snapshotFill(isolatedTile);

      // Case 3: Same-LOD neighbor stitching (level 8 west tile with sloped 5x5 heightmap -> level 8 east fill tile)
      const sameLodWest = getTile(8, 120, 60);
      const sameLodEast = getTile(8, 121, 60);
      makeHeightmapTile(sameLodWest, 5, 5, (u, v) => 100.0 + 200.0 * u + 160.0 * v);
      makeFillTile(sameLodEast, 0.0, 0.0);
      sameLodEast.data.fill = new ref.TerrainFillMesh(sameLodEast);
      sameLodEast.data.fill.westTiles = [sameLodWest];
      sameLodEast.data.fill.westMeshes = [sameLodWest.data.mesh];
      sameLodEast.data.fill.update(tileProvider, frameState);
      const sameLodStitch = snapshotFill(sameLodEast);

      // Case 3b: Same-LOD with exaggeration = 2.0
      frameState.verticalExaggeration = 2.0;
      frameState.verticalExaggerationRelativeHeight = 50.0;
      const sameLodEastExag = getTile(8, 121, 61);
      const sameLodWestExag = getTile(8, 120, 61);
      makeHeightmapTile(sameLodWestExag, 5, 5, (u, v) => 100.0 + 200.0 * u + 160.0 * v, 2.0);
      makeFillTile(sameLodEastExag, 0.0, 0.0);
      sameLodEastExag.data.fill = new ref.TerrainFillMesh(sameLodEastExag);
      sameLodEastExag.data.fill.westTiles = [sameLodWestExag];
      sameLodEastExag.data.fill.westMeshes = [sameLodWestExag.data.mesh];
      sameLodEastExag.data.fill.update(tileProvider, frameState);
      const sameLodExaggerated = snapshotFill(sameLodEastExag);
      frameState.verticalExaggeration = 1.0;
      frameState.verticalExaggerationRelativeHeight = 0.0;

      // Case 4: Coarser neighbor stitching (level 7 west tile 5x5 -> level 8 SW east fill tile, endpoint splits coarse segment)
      const coarseWest = getTile(7, 62, 32); // level 7
      const fineEast = getTile(8, 126, 65); // SW child of (7, 63, 32), shares southern half of coarseWest's east edge!
      // Use 6x6 heightmap on coarseWest so v=0.5 (fineEast's north corner) falls between grid rows (v=0.4 and v=0.6)!
      makeHeightmapTile(coarseWest, 6, 6, (u, v) => 120.0 + 80.0 * u + 200.0 * v);
      makeFillTile(fineEast, 0.0, 0.0);
      fineEast.data.fill = new ref.TerrainFillMesh(fineEast);
      fineEast.data.fill.westTiles = [coarseWest];
      fineEast.data.fill.westMeshes = [coarseWest.data.mesh];
      fineEast.data.fill.update(tileProvider, frameState);
      const coarserNeighborStitch = snapshotFill(fineEast);

      // Case 5: Finer neighbors stitching (TWO level 8 west tiles -> ONE level 7 east fill tile)
      const fineWestN = getTile(8, 131, 68); // north half of (7, 65, 34)'s east edge
      const fineWestS = getTile(8, 131, 69); // south half of (7, 65, 34)'s east edge
      const coarseEastFill = getTile(7, 66, 34); // shares west edge with fineWestN (north) and fineWestS (south)
      makeHeightmapTile(fineWestN, 3, 3, (u, v) => 300.0 + 40.0 * u + 60.0 * v);
      makeHeightmapTile(fineWestS, 3, 3, (u, v) => 180.0 + 40.0 * u + 120.0 * v);
      makeFillTile(coarseEastFill, 0.0, 0.0);
      coarseEastFill.data.fill = new ref.TerrainFillMesh(coarseEastFill);
      // westMeshes are ordered north-to-south (CCW)
      coarseEastFill.data.fill.westTiles = [fineWestN, fineWestS];
      coarseEastFill.data.fill.westMeshes = [fineWestN.data.mesh, fineWestS.data.mesh];
      coarseEastFill.data.fill.update(tileProvider, frameState);
      const finerNeighborsStitch = snapshotFill(coarseEastFill);

      // Case 6: Two adjacent edges (west + south) -> NE corner falls back to average of NW and SE!
      const twoEdgeCenter = getTile(8, 140, 70);
      const twoEdgeWest = getTile(8, 139, 70);
      const twoEdgeSouth = getTile(8, 140, 71);
      makeHeightmapTile(twoEdgeWest, 3, 3, (_u, v) => 100.0 + 100.0 * v); // SW=100, NW=200
      makeHeightmapTile(twoEdgeSouth, 3, 3, (u, _v) => 100.0 + 300.0 * u); // SW=100, SE=400
      makeFillTile(twoEdgeCenter, 0.0, 0.0);
      twoEdgeCenter.data.fill = new ref.TerrainFillMesh(twoEdgeCenter);
      twoEdgeCenter.data.fill.westTiles = [twoEdgeWest];
      twoEdgeCenter.data.fill.westMeshes = [twoEdgeWest.data.mesh];
      twoEdgeCenter.data.fill.southTiles = [twoEdgeSouth];
      twoEdgeCenter.data.fill.southMeshes = [twoEdgeSouth.data.mesh];
      twoEdgeCenter.data.fill.update(tileProvider, frameState);
      const twoEdgeCornerFallback = snapshotFill(twoEdgeCenter);

      // Case 7: Full BFS TerrainFillMesh.updateFillTiles across a 2-tile rendered set (loaded west -> fill east)
      const bfsWest = getTile(8, 150, 75);
      const bfsEast = getTile(8, 151, 75);
      makeHeightmapTile(bfsWest, 3, 3, (u, v) => 250.0 + 50.0 * u + 50.0 * v);
      makeFillTile(bfsEast, 0.0, 0.0);
      // Mark selection results on ancestors & rendered tiles so TerrainFillMesh.updateFillTiles traverses properly
      const frameNum = 42;
      quadtree._lastSelectionFrameNumber = frameNum;
      frameState.frameNumber = frameNum;
      const markAncestorsRefined = (t) => {
        let p = t.parent;
        while (p) {
          p._lastSelectionResultFrame = frameNum;
          p._lastSelectionResult = ref.TileSelectionResult.REFINED;
          p = p.parent;
        }
        t._lastSelectionResultFrame = frameNum;
        t._lastSelectionResult = ref.TileSelectionResult.RENDERED;
      };
      markAncestorsRefined(bfsWest);
      markAncestorsRefined(bfsEast);
      ref.TerrainFillMesh.updateFillTiles(tileProvider, [bfsWest, bfsEast], frameState, []);
      bfsEast.data.fill.update(tileProvider, frameState);
      const bfsPropagatedFill = snapshotFill(bfsEast);

      const glError = gl.getError();
      quadtree.destroy();
      shaderSet.destroy();
      context.destroy();

      return {
        lowDetailGrid,
        highDetailIsolated,
        sameLodStitch,
        sameLodExaggerated,
        coarserNeighborStitch,
        finerNeighborsStitch,
        twoEdgeCornerFallback,
        bfsPropagatedFill,
        glError,
      };
    })();

    return {
      globe: globeSection,
      globeSurfaceShaderSet: shaderSetSection,
      globeTranslucencyState: translucencySection,
      globeSurfaceTileProvider: tileProviderSection,
      terrainFillMesh: fillMeshSection,
    };
    } catch (e) {
      return { __error: `${phase}: ${e.name}: ${e.message}\n${e.stack}` };
    }
  });
  if (captured.__error) {
    throw new Error(captured.__error);
  }

  const fixture = {
    meta: {
      generator: 'tools/cesium/generate-scene-globe-terrain.mjs',
      cesium_engine_version: pkg.version,
      renderer: 'SwiftShader WebGL2 (headless Chromium)',
      modules: [
        'Scene/Globe',
        'Scene/GlobeSurfaceShaderSet',
        'Scene/GlobeSurfaceTileProvider',
        'Scene/GlobeTranslucencyState',
        'Scene/TerrainFillMesh',
      ],
    },
    ...captured,
  };

  mkdirSync(outDir, { recursive: true });
  writeFileSync(outPath, `${JSON.stringify(fixture, null, 2)}\n`);
  console.log(`Wrote ${outPath} (@cesium/engine ${pkg.version})`);
} finally {
  if (browser) await browser.close();
  server.close();
}
