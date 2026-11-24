// SwiftShader WebGL2 reference captures from pinned @cesium/engine 23.0.0
// for Cluster 4 (12 Scene Atmosphere, Shadows, Environment Maps, Panoramas &
// Ground Classification modules):
//  1. ClassificationPrimitive
//  2. GroundPrimitive
//  3. GroundPolylinePrimitive
//  4. InvertClassification
//  5. ShadowMap
//  6. SkyAtmosphere
//  7. Sun
//  8. SunPostProcess
//  9. CubeMapPanorama
// 10. DynamicEnvironmentMapManager
// 11. GoogleStreetViewCubeMapPanoramaProvider
// 12. SpecularEnvironmentCubeMap
//
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-atmosphere-gpu.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: [
    'Atmosphere',
    'BlendingState',
    'Camera',
    'ClassificationPrimitive',
    'ClassificationType',
    'CubeMapPanorama',
    'DynamicAtmosphereLightingType',
    'DynamicEnvironmentMapManager',
    'FrameState',
    'GlobeTranslucencyState',
    'GoogleStreetViewCubeMapPanoramaProvider',
    'GroundPolylinePrimitive',
    'GroundPrimitive',
    'InvertClassification',
    'JobScheduler',
    'Material',
    'MaterialAppearance',
    'PerInstanceColorAppearance',
    'PolylineColorAppearance',
    'PolylineMaterialAppearance',
    'SceneMode',
    'ShadowMap',
    'SkyAtmosphere',
    'SpecularEnvironmentCubeMap',
    'Sun',
    'SunLight',
    'SunPostProcess',
  ],
  Renderer: [
    'Buffer',
    'BufferUsage',
    'ClearCommand',
    'ComputeEngine',
    'Context',
    'ContextLimits',
    'CubeMap',
    'DrawCommand',
    'Framebuffer',
    'FramebufferManager',
    'Pass',
    'PassState',
    'PixelDatatype',
    'RenderState',
    'Sampler',
    'ShaderProgram',
    'ShaderSource',
    'Texture',
    'VertexArray',
  ],
  Core: [
    'ApproximateTerrainHeights',
    'BoundingRectangle',
    'BoundingSphere',
    'BoxGeometry',
    'Cartesian2',
    'Cartesian3',
    'Cartesian4',
    'Cartographic',
    'Color',
    'ColorGeometryInstanceAttribute',
    'ComponentDatatype',
    'Credit',
    'CullingVolume',
    'Ellipsoid',
    'GeographicProjection',
    'GeometryInstance',
    'GroundPolylineGeometry',
    'IndexDatatype',
    'JulianDate',
    'Matrix3',
    'Matrix4',
    'OrthographicOffCenterFrustum',
    'PerspectiveFrustum',
    'PixelFormat',
    'PrimitiveType',
    'Rectangle',
    'RectangleGeometry',
    'Resource',
    'ShowGeometryInstanceAttribute',
    'VertexFormat',
    'WebGLConstants',
  ],
};

const contents = Object.entries(modules)
  .flatMap(([dir, names]) =>
    names.map((n) => {
      const exportName = n.split('/').pop();
      return `export {default as ${exportName}} from '@cesium/engine/Source/${dir}/${n}.js';`;
    }),
  )
  .join('\n');

const bundle = await build({
  stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) },
  bundle: true,
  write: false,
  format: 'esm',
  platform: 'browser',
});

const approxTerrainHeights = JSON.parse(
  readFileSync(
    new URL('./node_modules/@cesium/engine/Source/Assets/approximateTerrainHeights.json', import.meta.url),
    'utf8',
  ),
);

const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') {
    res.setHeader('Content-Type', 'text/javascript');
    res.end(bundle.outputFiles[0].contents);
    return;
  }
  res.end('<!doctype html><title>Cluster 4 GPU Parity Captures</title>');
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

  const result = await page.evaluate(async (terrainHeights) => {
    try {
      const ref = await import('/reference.mjs');
      ref.ApproximateTerrainHeights._terrainHeights = terrainHeights;

      const fnv = (bytes) => {
        let h = 0xcbf29ce484222325n;
        for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
        return h.toString(16);
      };
      const c2 = (v) => [v.x, v.y];
      const c3 = (v) => [v.x, v.y, v.z];
      const c4 = (v) => [v.x, v.y, v.z, v.w];
      const m3 = (m) => Array.from(m);
      const m4 = (m) => Array.from(m);
      const bs = (b) => (b ? { center: c3(b.center), radius: b.radius } : null);
      const rect = (r) => (r ? [r.x, r.y, r.width, r.height] : null);

      const makeCanvas = (w, h) => {
        const c = document.createElement('canvas');
        c.width = w;
        c.height = h;
        return c;
      };
      const attrs = { alpha: true, depth: true, stencil: true, antialias: false, preserveDrawingBuffer: true };

      const snapRS = (s) =>
        s
          ? {
              cull: { enabled: s.cull.enabled, face: s.cull.face },
              depthTest: { enabled: s.depthTest.enabled, func: s.depthTest.func },
              depthMask: s.depthMask,
              colorMask: {
                red: s.colorMask.red,
                green: s.colorMask.green,
                blue: s.colorMask.blue,
                alpha: s.colorMask.alpha,
              },
              polygonOffset: {
                enabled: s.polygonOffset.enabled,
                factor: s.polygonOffset.factor,
                units: s.polygonOffset.units,
              },
              blending: {
                enabled: s.blending.enabled,
                functionSourceRgb: s.blending.functionSourceRgb,
                functionSourceAlpha: s.blending.functionSourceAlpha,
                functionDestinationRgb: s.blending.functionDestinationRgb,
                functionDestinationAlpha: s.blending.functionDestinationAlpha,
                equationRgb: s.blending.equationRgb,
                equationAlpha: s.blending.equationAlpha,
              },
              stencilTest: {
                enabled: s.stencilTest.enabled,
                frontFunction: s.stencilTest.frontFunction,
                backFunction: s.stencilTest.backFunction,
                reference: s.stencilTest.reference,
                mask: s.stencilTest.mask,
                frontOperation: {
                  fail: s.stencilTest.frontOperation.fail,
                  zFail: s.stencilTest.frontOperation.zFail,
                  zPass: s.stencilTest.frontOperation.zPass,
                },
                backOperation: {
                  fail: s.stencilTest.backOperation.fail,
                  zFail: s.stencilTest.backOperation.zFail,
                  zPass: s.stencilTest.backOperation.zPass,
                },
              },
              stencilMask: s.stencilMask,
              viewport: s.viewport ? [s.viewport.x, s.viewport.y, s.viewport.width, s.viewport.height] : null,
            }
          : null;

      const makeCamera = (w, h, pos, dir, up, near = 1.0, far = 10000.0, mode = ref.SceneMode.SCENE3D) => {
        const scene = {
          drawingBufferWidth: w,
          drawingBufferHeight: h,
          mapProjection: new ref.GeographicProjection(),
          mode,
          ellipsoid: ref.Ellipsoid.WGS84,
          screenSpaceCameraController: {},
        };
        const cam = new ref.Camera(scene);
        cam.position = new ref.Cartesian3(...pos);
        cam.direction = ref.Cartesian3.normalize(new ref.Cartesian3(...dir), new ref.Cartesian3());
        cam.up = ref.Cartesian3.normalize(new ref.Cartesian3(...up), new ref.Cartesian3());
        cam.right = ref.Cartesian3.normalize(
          ref.Cartesian3.cross(cam.direction, cam.up, new ref.Cartesian3()),
          new ref.Cartesian3(),
        );
        cam.up = ref.Cartesian3.normalize(
          ref.Cartesian3.cross(cam.right, cam.direction, new ref.Cartesian3()),
          new ref.Cartesian3(),
        );
        if (mode === ref.SceneMode.SCENE2D) {
          cam.frustum = new ref.OrthographicOffCenterFrustum({
            left: -500000.0,
            right: 500000.0,
            bottom: -500000.0,
            top: 500000.0,
            near,
            far,
          });
        } else {
          cam.frustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: w / h, near, far });
        }
        cam.update(mode);
        return cam;
      };

      const makeFrameState = (context, camera, opts = {}) => {
        const time = opts.time ?? ref.JulianDate.fromIso8601('2025-06-21T12:00:00Z');
        const mapProjection = opts.mapProjection ?? new ref.GeographicProjection();
        const mode = opts.mode ?? ref.SceneMode.SCENE3D;
        const creditDisplay = {
          credits: [],
          addCreditToNextFrame(c) {
            this.credits.push(c.html);
          },
        };
        const frameState = new ref.FrameState(context, creditDisplay, new ref.JobScheduler());
        frameState.camera = camera;
        frameState.time = time;
        frameState.mode = mode;
        frameState.morphTime = mode === ref.SceneMode.SCENE3D ? 1.0 : mode === ref.SceneMode.MORPHING ? 0.5 : 0.0;
        frameState.mapProjection = mapProjection;
        frameState.scene3DOnly = opts.scene3DOnly ?? false;
        frameState.verticalExaggeration = opts.verticalExaggeration ?? 1.0;
        frameState.verticalExaggerationRelativeHeight = opts.verticalExaggerationRelativeHeight ?? 0.0;
        frameState.invertClassification = opts.invertClassification ?? false;
        frameState.useLogDepth = false;
        frameState.passes.render = opts.render ?? true;
        frameState.passes.pick = opts.pick ?? false;
        frameState.globeTranslucencyState = new ref.GlobeTranslucencyState();
        frameState.atmosphere = opts.atmosphere ?? new ref.Atmosphere();
        frameState.light = new ref.SunLight();
        frameState.backgroundColor = ref.Color.BLACK;
        frameState.cullingVolume = camera.frustum.computeCullingVolume(camera.positionWC, camera.directionWC, camera.upWC);
        frameState.shadowState.nearPlane = opts.shadowNear ?? 1.0;
        frameState.shadowState.farPlane = opts.shadowFar ?? 5000.0;
        frameState.shadowState.closestObjectSize = opts.closestObjectSize ?? 1000.0;
        if (opts.translucent) {
          Object.defineProperty(frameState.globeTranslucencyState, 'translucent', { get: () => true });
        }
        context.uniformState.update(frameState);
        context.uniformState.updateCamera(camera);
        return frameState;
      };

      const cleanDefines = (arr) => (arr ?? []).filter((d) => d && d.length > 0);

      const snapCmd = (cmd) => ({
        pass: cmd.pass,
        primitiveType: cmd.primitiveType,
        count: cmd.count ?? null,
        offset: cmd.offset ?? 0,
        pickOnly: !!cmd.pickOnly,
        pickId: cmd.pickId ?? null,
        modelMatrix: cmd.modelMatrix ? m4(cmd.modelMatrix) : null,
        boundingVolume: bs(cmd.boundingVolume),
        renderState: snapRS(cmd.renderState),
        vsDefines: cleanDefines(cmd.shaderProgram?.vertexShaderSource?.defines),
        fsDefines: cleanDefines(cmd.shaderProgram?.fragmentShaderSource?.defines),
        vsHasShow: (cmd.shaderProgram?.vertexShaderSource?.sources ?? []).join('\n').includes('czm_batchTable_show'),
        vsHasPickColor: (cmd.shaderProgram?.vertexShaderSource?.sources ?? []).join('\n').includes('czm_batchTable_pickColor'),
        fsHasPickColor: (cmd.shaderProgram?.fragmentShaderSource?.sources ?? []).join('\n').includes('czm_batchTable_pickColor'),
        uniformNames: cmd.uniformMap ? Object.keys(cmd.uniformMap).sort() : [],
        vaCount: cmd.vertexArray?.numberOfAttributes ?? 0,
        hasIndexBuffer: !!cmd.vertexArray?.indexBuffer,
      });

      // ======================================================================
      // 1. SkyAtmosphere
      // ======================================================================
      const skyAtmosphere = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(32, 32, [7000000, 1000000, 500000], [-7, -1, -0.5], [0, 0, 1]);
        const camCV = makeCamera(32, 32, [0, 0, 1000000], [0, 0, -1], [0, 1, 0], 1.0, 10000.0, ref.SceneMode.COLUMBUS_VIEW);
        const sky = new ref.SkyAtmosphere();
        sky.setDynamicLighting(ref.DynamicAtmosphereLightingType.SUNLIGHT);

        const cases = [];
        const runCase = (name, setup, globeArg, frameOpts) => {
          setup(sky);
          const activeCam = frameOpts?.mode === ref.SceneMode.COLUMBUS_VIEW ? camCV : cam;
          const fs = makeFrameState(context, activeCam, frameOpts);
          const cmd = sky.update(fs, globeArg);
          let executed = false;
          if (cmd) {
            const passState = new ref.PassState(context);
            passState.viewport = new ref.BoundingRectangle(0, 0, 32, 32);
            context.uniformState.updatePass(ref.Pass.ENVIRONMENT);
            cmd.execute(context, passState);
            executed = true;
          }
          cases.push({
            name,
            hasCommand: !!cmd,
            executed,
            modelMatrix: cmd ? m4(cmd.modelMatrix) : null,
            vsDefines: cmd ? cleanDefines(cmd.shaderProgram.vertexShaderSource.defines) : null,
            fsDefines: cmd ? cleanDefines(cmd.shaderProgram.fragmentShaderSource.defines) : null,
            renderState: cmd ? snapRS(cmd.renderState) : null,
            uniforms: cmd
              ? {
                  u_radiiAndDynamicAtmosphereColor: c3(cmd.uniformMap.u_radiiAndDynamicAtmosphereColor()),
                  u_hsbShift: c3(cmd.uniformMap.u_hsbShift()),
                  u_atmosphereLightIntensity: cmd.uniformMap.u_atmosphereLightIntensity(),
                  u_atmosphereRayleighCoefficient: c3(cmd.uniformMap.u_atmosphereRayleighCoefficient()),
                  u_atmosphereMieCoefficient: c3(cmd.uniformMap.u_atmosphereMieCoefficient()),
                  u_atmosphereRayleighScaleHeight: cmd.uniformMap.u_atmosphereRayleighScaleHeight(),
                  u_atmosphereMieScaleHeight: cmd.uniformMap.u_atmosphereMieScaleHeight(),
                  u_atmosphereMieAnisotropy: cmd.uniformMap.u_atmosphereMieAnisotropy(),
                }
              : null,
            glError: gl.getError(),
          });
        };

        runCase('default-3d-globe', () => {}, { show: true }, {});
        runCase('per-fragment', (s) => { s.perFragmentAtmosphere = true; }, { show: true }, {});
        runCase('no-globe', (s) => { s.perFragmentAtmosphere = false; }, undefined, {});
        runCase(
          'translucent-color-correct',
          (s) => { s.perFragmentAtmosphere = false; s.hueShift = 0.25; s.brightnessShift = -0.1; },
          { show: true },
          { translucent: true },
        );
        runCase('morphing', () => {}, { show: true }, { mode: ref.SceneMode.MORPHING });
        runCase('columbus-view-none', () => {}, { show: true }, { mode: ref.SceneMode.COLUMBUS_VIEW });
        runCase('pick-only-none', () => {}, { show: true }, { render: false, pick: true });
        runCase('hidden-none', (s) => { s.show = false; }, { show: true }, {});

        sky.destroy();
        context.destroy();
        return {
          camera: {
            view: m4(cam.viewMatrix),
            inverseView: m4(cam.inverseViewMatrix),
            position: c3(cam.positionWC),
            direction: c3(cam.directionWC),
            up: c3(cam.upWC),
            right: c3(cam.rightWC),
          },
          cases,
        };
      })();

      // ======================================================================
      // 2. Sun
      // ======================================================================
      const sun = (() => {
        const canvas = makeCanvas(256, 128);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const computeEngine = new ref.ComputeEngine(context);
        const cam = makeCamera(256, 128, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);
        const camCV = makeCamera(256, 128, [0, 0, 1000000], [0, 0, -1], [0, 1, 0], 1.0, 10000.0, ref.SceneMode.COLUMBUS_VIEW);
        camCV.update(ref.SceneMode.COLUMBUS_VIEW);
        const cam2D = makeCamera(256, 128, [0, 0, 1000000], [0, 0, -1], [0, 1, 0], 1.0, 10000.0, ref.SceneMode.SCENE2D);
        cam2D.update(ref.SceneMode.SCENE2D);
        const time = ref.JulianDate.fromIso8601('2025-06-21T12:00:00Z');
        const sunObj = new ref.Sun();

        const cases = [];
        const runSun = (name, vp, useHdr, mode, glow) => {
          if (glow !== undefined) sunObj.glowFactor = glow;
          const activeCam =
            mode === ref.SceneMode.COLUMBUS_VIEW ? camCV : mode === ref.SceneMode.SCENE2D ? cam2D : cam;
          const fs = makeFrameState(context, activeCam, { time, mode });
          const ps = new ref.PassState(context);
          ps.viewport = new ref.BoundingRectangle(...vp);
          const cmds = sunObj.update(fs, ps, useHdr);
          let computeRan = false;
          let centerPixel = null;
          let cornerPixel = null;
          let hash = null;
          let radiusTS = null;
          if (cmds?.computeCommand) {
            radiusTS = cmds.computeCommand.uniformMap.u_radiusTS();
            computeEngine.execute(cmds.computeCommand);
            computeRan = true;
            if (!useHdr) {
              const tex = sunObj._texture;
              const fb = new ref.Framebuffer({ context, colorTextures: [tex], destroyAttachments: false });
              const pixels = context.readPixels({ framebuffer: fb, width: tex.width, height: tex.height });
              fb.destroy();
              const mid = ((tex.height >> 1) * tex.width + (tex.width >> 1)) * 4;
              centerPixel = [pixels[mid], pixels[mid + 1], pixels[mid + 2], pixels[mid + 3]];
              cornerPixel = [pixels[0], pixels[1], pixels[2], pixels[3]];
              hash = fnv(pixels);
            }
          }
          if (cmds?.drawCommand) {
            context.uniformState.updatePass(ref.Pass.ENVIRONMENT);
            cmds.drawCommand.execute(context, ps);
          }
          cases.push({
            name,
            viewport: vp,
            useHdr,
            mode,
            glowFactor: sunObj.glowFactor,
            hasCommands: !!cmds,
            computeRan,
            radiusTS,
            glowLengthTS: sunObj._glowLengthTS ?? null,
            size: cmds ? sunObj._size : null,
            boundingVolume: cmds ? bs(cmds.drawCommand.boundingVolume) : null,
            texture: sunObj._texture
              ? {
                  width: sunObj._texture.width,
                  height: sunObj._texture.height,
                  pixelFormat: sunObj._texture.pixelFormat,
                  pixelDatatype: sunObj._texture.pixelDatatype,
                  sampler: {
                    minificationFilter: sunObj._texture.sampler.minificationFilter,
                    magnificationFilter: sunObj._texture.sampler.magnificationFilter,
                    wrapS: sunObj._texture.sampler.wrapS,
                    wrapT: sunObj._texture.sampler.wrapT,
                  },
                }
              : null,
            renderState: cmds ? snapRS(cmds.drawCommand.renderState) : null,
            centerPixel,
            cornerPixel,
            hash,
            glError: gl.getError(),
          });
        };

        runSun('3d-ldr-256x128', [0, 0, 256, 128], false, ref.SceneMode.SCENE3D, 1.0);
        runSun('3d-ldr-cached', [0, 0, 256, 128], false, ref.SceneMode.SCENE3D, undefined);
        runSun('3d-glow-2.5', [0, 0, 256, 128], false, ref.SceneMode.SCENE3D, 2.5);
        runSun('3d-hdr', [0, 0, 256, 128], true, ref.SceneMode.SCENE3D, 1.0);
        runSun('cv-ldr', [0, 0, 256, 128], false, ref.SceneMode.COLUMBUS_VIEW, 0.0);
        runSun('2d-none', [0, 0, 256, 128], false, ref.SceneMode.SCENE2D, undefined);

        sunObj.destroy();
        computeEngine.destroy();
        context.destroy();
        return {
          sunPositionWC: c3(
            (() => {
              const c = makeCanvas(4, 4);
              const ctx = new ref.Context(c, { webgl: attrs });
              const fs = makeFrameState(ctx, cam, { time });
              const p = ref.Cartesian3.clone(ctx.uniformState.sunPositionWC);
              ctx.destroy();
              return p;
            })(),
          ),
          cases,
        };
      })();

      // ======================================================================
      // 3. SunPostProcess
      // ======================================================================
      const sunPostProcess = (() => {
        const canvas = makeCanvas(128, 64);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const time = ref.JulianDate.fromIso8601('2025-06-21T12:00:00Z');
        // Point camera directly toward the sun so sun projects inside viewport
        const tmpCam = makeCamera(128, 64, [0, 0, 0], [1, 0, 0], [0, 0, 1]);
        makeFrameState(context, tmpCam, { time });
        const sunPos = ref.Cartesian3.clone(context.uniformState.sunPositionWC);
        const sunDir = ref.Cartesian3.normalize(sunPos, new ref.Cartesian3());
        const cam = makeCamera(128, 64, [0, 0, 0], c3(sunDir), [0, 0, 1], 1.0, 1e12);
        makeFrameState(context, cam, { time });

        const spp = new ref.SunPostProcess();
        const passState = new ref.PassState(context);
        passState.viewport = new ref.BoundingRectangle(0, 0, 128, 64);

        const sceneFbo = spp.update(passState);
        spp.clear(context, passState, ref.Color.BLACK);
        spp.execute(context);

        const outTex = new ref.Texture({ context, width: 128, height: 64 });
        const outFbo = new ref.Framebuffer({ context, colorTextures: [outTex], destroyAttachments: false });
        spp.copy(context, outFbo);
        const copyGlError = gl.getError();

        const stages = [];
        for (let i = 0; i < spp.length; i++) {
          const st = spp.get(i);
          const u = {};
          for (const [k, v] of Object.entries(st._uniforms ?? {})) {
            const val = typeof v === 'function' ? v() : v;
            if (val instanceof ref.Cartesian2) u[k] = c2(val);
            else if (typeof val === 'number') u[k] = val;
            else u[k] = val ? 'texture' : null;
          }
          stages.push({
            textureScale: st.textureScale,
            forcePowerOfTwo: st.forcePowerOfTwo,
            sampleMode: st.sampleMode,
            outputDims: st.outputTexture ? [st.outputTexture.width, st.outputTexture.height] : null,
            scissorRectangle: rect(st.scissorRectangle),
            uniforms: u,
          });
        }

        const out = {
          viewport: [0, 0, 128, 64],
          camera: {
            view: m4(cam.viewMatrix),
            inverseView: m4(cam.inverseViewMatrix),
            position: c3(cam.positionWC),
            direction: c3(cam.directionWC),
            up: c3(cam.upWC),
            right: c3(cam.rightWC),
            near: cam.frustum.near,
            far: cam.frustum.far,
            fov: cam.frustum.fov,
            aspectRatio: cam.frustum.aspectRatio,
          },
          sunPositionWC: c3(sunPos),
          sceneFboColorDims: [sceneFbo.getColorTexture(0).width, sceneFbo.getColorTexture(0).height],
          uCenter: c2(spp._uCenter),
          uRadius: spp._uRadius,
          stages,
          glError: copyGlError,
        };
        outFbo.destroy();
        outTex.destroy();
        spp.destroy();
        context.destroy();
        return out;
      })();

      // ======================================================================
      // 4. CubeMapPanorama
      // ======================================================================
      const cubeMapPanorama = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(32, 32, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);

        const makeFace = (rgba) => {
          const data = new Uint8ClampedArray(4 * 4 * 4);
          for (let i = 0; i < 16; i++) data.set(rgba, i * 4);
          return new ImageData(data, 4, 4);
        };
        const sources = {
          positiveX: makeFace([255, 0, 0, 255]),
          negativeX: makeFace([0, 255, 0, 255]),
          positiveY: makeFace([0, 0, 255, 255]),
          negativeY: makeFace([255, 255, 0, 255]),
          positiveZ: makeFace([255, 0, 255, 255]),
          negativeZ: makeFace([0, 255, 255, 255]),
        };
        const transform = new ref.Matrix3(0, 1, 0, -1, 0, 0, 0, 0, 1);

        // Case A: returnCommand=true, with transform, LDR -> HDR switch
        const p1 = new ref.CubeMapPanorama({ sources, transform, returnCommand: true, credit: 'Test Credit' });
        const fs1 = makeFrameState(context, cam);
        const cmdLdr = p1.update(fs1, false);
        const passState = new ref.PassState(context);
        passState.viewport = new ref.BoundingRectangle(0, 0, 32, 32);
        context.uniformState.updatePass(ref.Pass.ENVIRONMENT);
        cmdLdr.execute(context, passState);
        const ldrSnap = {
          command: snapCmd(cmdLdr),
          usesPanoramaVs: cmdLdr.shaderProgram.vertexShaderSource.sources.join('\n').includes('u_cubeMapPanoramaTransform'),
          cubeMap: {
            width: p1._cubeMap.width,
            height: p1._cubeMap.height,
            pixelFormat: p1._cubeMap.pixelFormat,
            pixelDatatype: p1._cubeMap.pixelDatatype,
            flipY: p1._cubeMap._flipY,
          },
          creditsAdded: fs1.creditDisplay.credits.slice(),
          glError: gl.getError(),
        };

        const cmdHdr = p1.update(fs1, true);
        cmdHdr.execute(context, passState);
        const hdrSnap = {
          command: snapCmd(cmdHdr),
          glError: gl.getError(),
        };
        p1.destroy();

        // Case B: returnCommand=false, no transform -> SkyBoxVS, panoramaCommandList once, credit added
        const p2 = new ref.CubeMapPanorama({ sources, returnCommand: false, credit: 'Pano Credit' });
        const fs2a = makeFrameState(context, cam);
        const ret2a = p2.update(fs2a, false);
        const fs2b = makeFrameState(context, cam);
        const ret2b = p2.update(fs2b, false);
        const listSnap = {
          firstReturnUndefined: ret2a === undefined,
          firstListLen: fs2a.panoramaCommandList.length,
          firstCredits: fs2a.creditDisplay.credits.slice(),
          usesSkyBoxVs: !fs2a.panoramaCommandList[0].shaderProgram.vertexShaderSource.sources
            .join('\n')
            .includes('u_cubeMapPanoramaTransform'),
          secondReturnUndefined: ret2b === undefined,
          secondListLen: fs2b.panoramaCommandList.length,
          secondCredits: fs2b.creditDisplay.credits.slice(),
          glError: gl.getError(),
        };
        p2.destroy();
        context.destroy();
        return { transform: m3(transform), ldrSnap, hdrSnap, listSnap };
      })();

      // ======================================================================
      // 5. InvertClassification
      // ======================================================================
      const invertClassification = (() => {
        const canvas = makeCanvas(16, 12);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(16, 12, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);
        const fsInit = makeFrameState(context, cam);
        fsInit.invertClassificationColor = new ref.Color(0.2, 0.3, 0.4, 0.5);
        context.uniformState.update(fsInit);
        const inv = new ref.InvertClassification();

        const snapInv = (label) => ({
          label,
          numSamples: inv._numSamples,
          colorDims: inv._fbo.getColorTexture()
            ? [inv._fbo.getColorTexture().width, inv._fbo.getColorTexture().height]
            : null,
          classifiedDims: inv._fboClassified.getColorTexture()
            ? [inv._fboClassified.getColorTexture().width, inv._fboClassified.getColorTexture().height]
            : null,
          depthStencilTex: inv._depthStencilTexture
            ? {
                width: inv._depthStencilTexture.width,
                height: inv._depthStencilTexture.height,
                pixelFormat: inv._depthStencilTexture.pixelFormat,
                pixelDatatype: inv._depthStencilTexture.pixelDatatype,
              }
            : null,
          depthStencilRb: inv._depthStencilRenderbuffer
            ? {
                width: inv._depthStencilRenderbuffer.width,
                height: inv._depthStencilRenderbuffer.height,
                format: inv._depthStencilRenderbuffer.format,
                numSamples: inv._depthStencilRenderbuffer.numSamples,
              }
            : null,
          unclassifiedCmd: inv._unclassifiedCommand ? snapCmd(inv._unclassifiedCommand) : null,
          classifiedCmd: inv._classifiedCommand ? snapCmd(inv._classifiedCommand) : null,
          translucentCmd: inv._translucentCommand ? snapCmd(inv._translucentCommand) : null,
          unclassifiedFsHasDepth: inv._unclassifiedCommand?.shaderProgram.fragmentShaderSource.sources
            .join('\n')
            .includes('gl_FragDepth'),
          glError: gl.getError(),
        });

        const passState = new ref.PassState(context);
        passState.viewport = new ref.BoundingRectangle(0, 0, 16, 12);
        const steps = [];

        // Step 1: Translucent mode, 1 sample
        inv.update(context, 1, undefined);
        inv.clear(context, passState);
        inv.executeClassified(context, passState);
        inv.executeUnclassified(context, passState);
        steps.push(snapInv('translucent-1x'));

        // Step 2: Opaque mode (previousFramebuffer defined)
        const globeFboMgr = new ref.FramebufferManager({ depthStencil: true, supportsDepthTexture: true });
        globeFboMgr.update(context, 16, 12, 1);
        inv.previousFramebuffer = globeFboMgr.framebuffer;
        inv.update(context, 1, globeFboMgr);
        inv.clear(context, passState);
        inv.executeClassified(context, passState);
        inv.executeUnclassified(context, passState);
        steps.push(snapInv('opaque-1x'));

        // Step 3: Resize to 24x16 in translucent mode
        canvas.width = 24;
        canvas.height = 16;
        inv.previousFramebuffer = undefined;
        inv.update(context, 1, undefined);
        steps.push(snapInv('resized-24x16'));

        // Step 4: Translucent mode, 4x MSAA
        inv.update(context, 4, undefined);
        passState.viewport = new ref.BoundingRectangle(0, 0, 24, 16);
        inv.clear(context, passState);
        inv.executeClassified(context, passState);
        inv.executeUnclassified(context, passState);
        steps.push(snapInv('translucent-4x-msaa'));

        globeFboMgr.destroy();
        inv.destroy();
        context.destroy();
        return { steps };
      })();

      // ======================================================================
      // 6. SpecularEnvironmentCubeMap
      // ======================================================================
      const specularEnvironmentCubeMap = (() => {
        const canvas = makeCanvas(16, 16);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;

        const makeLevel = (size, fill) => {
          const faces = {};
          for (const name of ref.CubeMap.faceNames()) {
            const arr = new Uint8Array(size * size * 4);
            arr.fill(fill);
            faces[name] = {
              width: size,
              height: size,
              pixelFormat: ref.PixelFormat.RGBA,
              pixelDatatype: ref.PixelDatatype.UNSIGNED_BYTE,
              arrayBufferView: arr,
            };
          }
          return faces;
        };

        const runSpec = (name, url, levels) => {
          const uploads = [];
          const origTexImage = gl.texImage2D.bind(gl);
          gl.texImage2D = function (...a) {
            uploads.push({ target: a[0], level: a[1], width: a[3], height: a[4], isNull: a[a.length - 1] === null });
            return origTexImage(...a);
          };
          const spec = new ref.SpecularEnvironmentCubeMap(url);
          spec._cubeMapBuffers = levels;
          spec._loading = false;
          spec.update({ context });
          gl.texImage2D = origTexImage;
          const tex = spec.texture;
          const out = {
            name,
            supported: ref.SpecularEnvironmentCubeMap.isSupported(context),
            ready: spec.ready,
            maximumMipmapLevel: spec.maximumMipmapLevel,
            texture: {
              width: tex.width,
              height: tex.height,
              pixelFormat: tex.pixelFormat,
              pixelDatatype: tex.pixelDatatype,
              flipY: tex._flipY,
              sampler: {
                minificationFilter: tex.sampler.minificationFilter,
                magnificationFilter: tex.sampler.magnificationFilter,
                wrapS: tex.sampler.wrapS,
                wrapT: tex.sampler.wrapT,
              },
            },
            uploadCount: uploads.length,
            uploadLevels: [...new Set(uploads.map((u) => u.level))].sort((a, b) => a - b),
            nullUploadLevels: [...new Set(uploads.filter((u) => u.isNull).map((u) => u.level))].sort((a, b) => a - b),
            glError: gl.getError(),
          };
          // Second update cleans up buffers
          spec.update({ context });
          out.buffersClearedOnSecondUpdate = spec._cubeMapBuffers === undefined;
          spec.destroy();
          out.destroyed = spec.isDestroyed();
          return out;
        };

        const fullChain = runSpec('full-3-level', 'full.ktx2', [makeLevel(4, 200), makeLevel(2, 128), makeLevel(1, 64)]);
        const truncatedChain = runSpec('truncated-1-level', 'trunc.ktx2', [makeLevel(4, 180)]);
        context.destroy();
        return { fullChain, truncatedChain };
      })();

      // ======================================================================
      // 7. DynamicEnvironmentMapManager
      // ======================================================================
      const dynamicEnvironmentMapManager = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const computeEngine = new ref.ComputeEngine(context);
        const cam = makeCamera(32, 32, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);
        const time = ref.JulianDate.fromIso8601('2025-06-21T12:00:00Z');

        // Reset static queue state
        ref.DynamicEnvironmentMapManager._activeComputeCommandCount = 0;
        ref.DynamicEnvironmentMapManager._nextFrameCommandQueue = [];

        const mgr = new ref.DynamicEnvironmentMapManager({
          mipmapLevels: 3,
          brightness: 1.1,
          gamma: 1.0,
          saturation: 1.0,
          atmosphereScatteringIntensity: 2.5,
        });
        const pos = ref.Cartesian3.fromDegrees(0, 0, 1000);
        mgr.position = pos;

        const atmo = new ref.Atmosphere();
        atmo.dynamicLighting = ref.DynamicAtmosphereLightingType.SUNLIGHT;

        const frames = [];
        let rawIrradianceFloats = null;

        for (let step = 0; step < 12; step++) {
          const fs = makeFrameState(context, cam, { time, atmosphere: atmo });
          // Intercept readPixels when SH coefficients are read back
          const origRead = context.readPixels.bind(context);
          context.readPixels = function (readState) {
            const data = origRead(readState);
            if (readState.width === 3 && readState.height === 3 && data instanceof Float32Array) {
              rawIrradianceFloats = Array.from(data);
            }
            return data;
          };
          mgr.update(fs);
          context.readPixels = origRead;

          const cmds = fs.commandList.slice();
          const cmdSummaries = cmds.map((c) => ({
            outputDims: c.outputTexture ? [c.outputTexture.width, c.outputTexture.height] : null,
            outputDatatype: c.outputTexture?.pixelDatatype ?? null,
            fsDefines: cleanDefines(c.fragmentShaderSource?.defines),
            persists: !!c.persists,
            roughness: c.uniformMap?.u_roughness ? c.uniformMap.u_roughness() : null,
          }));
          for (const c of cmds) {
            computeEngine.execute(c);
          }

          frames.push({
            step,
            queuedThisFrame: cmds.length,
            backlogAfterUpdate: ref.DynamicEnvironmentMapManager._nextFrameCommandQueue.length,
            activeAfterExecute: ref.DynamicEnvironmentMapManager._activeComputeCommandCount,
            radianceMapDirty: mgr._radianceMapDirty,
            convolutionsCommandsDirty: mgr._convolutionsCommandsDirty,
            irradianceCommandDirty: mgr._irradianceCommandDirty,
            irradianceTextureDirty: mgr._irradianceTextureDirty,
            shDirty: mgr._sphericalHarmonicCoefficientsDirty,
            shouldRegenerateShaders: mgr.shouldRegenerateShaders,
            radianceCubeMap: mgr.radianceCubeMap
              ? {
                  width: mgr.radianceCubeMap.width,
                  height: mgr.radianceCubeMap.height,
                  minFilter: mgr.radianceCubeMap.sampler.minificationFilter,
                }
              : null,
            cmdSummaries,
            glError: gl.getError(),
          });

          if (rawIrradianceFloats && !mgr.shouldRegenerateShaders && step > 4) {
            break;
          }
        }

        const out = {
          position: c3(pos),
          radiiAndDynamicAtmosphereColor: c3(mgr._radiiAndDynamicAtmosphereColor),
          mipmapLevels: mgr.maximumMipmapLevel,
          textureDimensions: c2(mgr._textureDimensions),
          atmosphereScatteringIntensity: mgr.atmosphereScatteringIntensity,
          rawIrradianceFloats,
          shCoefficients: mgr.sphericalHarmonicCoefficients.map(c3),
          frames,
        };
        mgr.destroy();
        computeEngine.destroy();
        context.destroy();
        return out;
      })();

      // ======================================================================
      // 8. ClassificationPrimitive
      // ======================================================================
      const classificationPrimitive = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(32, 32, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);

        const makePrim = (opts) =>
          new ref.ClassificationPrimitive({
            geometryInstances: new ref.GeometryInstance({
              geometry: ref.BoxGeometry.fromDimensions({
                dimensions: new ref.Cartesian3(10, 10, 10),
                vertexFormat: ref.VertexFormat.POSITION_ONLY,
              }),
              modelMatrix: ref.Matrix4.fromTranslation(new ref.Cartesian3(6378137, 0, 0)),
              id: 'box-1',
              attributes: {
                color: ref.ColorGeometryInstanceAttribute.fromColor(new ref.Color(1, 0, 0, 0.5)),
                show: new ref.ShowGeometryInstanceAttribute(true),
              },
            }),
            asynchronous: false,
            allowPicking: true,
            classificationType: opts.classificationType ?? ref.ClassificationType.BOTH,
            debugShowShadowVolume: opts.debugShowShadowVolume ?? false,
          });

        const runClass = (name, primOpts, frameOpts) => {
          const p = makePrim(primOpts);
          const fs = makeFrameState(context, cam, frameOpts);
          p.update(fs);
          for (const cb of fs.afterRender) cb();
          const cmds = fs.commandList.map(snapCmd);
          // Execute render commands on SwiftShader WebGL2
          const passState = new ref.PassState(context);
          passState.viewport = new ref.BoundingRectangle(0, 0, 32, 32);
          for (const c of fs.commandList) {
            context.uniformState.updatePass(c.pass);
            c.execute(context, passState);
          }
          const out = { name, ready: p.ready, commandCount: cmds.length, commands: cmds, glError: gl.getError() };
          p.destroy();
          return out;
        };

        const cases = [
          runClass('both-render', {}, { render: true, pick: false }),
          runClass('terrain-only-render', { classificationType: ref.ClassificationType.TERRAIN }, { render: true, pick: false }),
          runClass('tiles-only-render', { classificationType: ref.ClassificationType.CESIUM_3D_TILE }, { render: true, pick: false }),
          runClass('both-invert-classification', {}, { render: true, pick: false, invertClassification: true }),
          runClass('both-pick', {}, { render: false, pick: true }),
          runClass('tiles-only-pick', { classificationType: ref.ClassificationType.CESIUM_3D_TILE }, { render: false, pick: true }),
          runClass('debug-shadow-volume', { debugShowShadowVolume: true }, { render: true, pick: false }),
        ];
        context.destroy();
        return { cases };
      })();

      // ======================================================================
      // 9. GroundPrimitive
      // ======================================================================
      const groundPrimitive = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(32, 32, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);

        const runGround = (name, rectDeg, useMaterial, frameOpts, extraOpts = {}) => {
          const r = ref.Rectangle.fromDegrees(...rectDeg);
          const inst = new ref.GeometryInstance({
            geometry: new ref.RectangleGeometry({ rectangle: r }),
            id: name,
            attributes: useMaterial
              ? {}
              : { color: ref.ColorGeometryInstanceAttribute.fromColor(new ref.Color(0, 1, 0, 0.5)) },
          });
          const appearance = useMaterial
            ? new ref.MaterialAppearance({ material: ref.Material.fromType('Color', { color: ref.Color.BLUE }) })
            : undefined;
          const gp = new ref.GroundPrimitive({
            geometryInstances: inst,
            appearance,
            asynchronous: false,
            classificationType: extraOpts.classificationType ?? ref.ClassificationType.BOTH,
            debugShowShadowVolume: extraOpts.debugShowShadowVolume ?? false,
          });
          const fs = makeFrameState(context, cam, frameOpts);
          gp.update(fs);
          for (const cb of fs.afterRender) cb();
          const cmds = fs.commandList.map(snapCmd);
          const globeMinAlt =
            fs.commandList[0]?.uniformMap?.u_globeMinimumAltitude
              ? fs.commandList[0].uniformMap.u_globeMinimumAltitude()
              : null;
          // Also check 2D/CV mode update on the same primitive!
          const cam2d = makeCamera(32, 32, [0, 0, 1000000], [0, 0, -1], [0, 1, 0], 1.0, 10000.0, ref.SceneMode.SCENE2D);
          const fs2d = makeFrameState(context, cam2d, { ...frameOpts, mode: ref.SceneMode.SCENE2D });
          gp.update(fs2d);
          const cmds2d = fs2d.commandList.map(snapCmd);
          const out = {
            name,
            rectDeg,
            ready: gp.ready,
            minHeight: gp._minHeight,
            maxHeight: gp._maxHeight,
            useFragmentCulling: gp._useFragmentCulling,
            boundingSphere: bs(gp.getBoundingSphere(name)),
            boundingVolume2D: gp._boundingVolumes2D[0] ? bs(gp._boundingVolumes2D[0]) : null,
            globeMinAlt,
            commands3D: cmds,
            commands2D: cmds2d,
            glError: gl.getError(),
          };
          gp.destroy();
          return out;
        };

        const cases = [
          runGround('planar-color-exaggerated', [-1.4, 0.5, -1.1, 0.7], false, {
            render: true,
            pick: false,
            verticalExaggeration: 2.0,
            verticalExaggerationRelativeHeight: 100.0,
          }),
          runGround('spherical-material', [-10.0, 10.0, 10.0, 30.0], true, { render: true, pick: false }),
          runGround('planar-pick', [-1.4, 0.5, -1.1, 0.7], false, { render: false, pick: true }),
          runGround(
            'debug-shadow-volume',
            [-1.4, 0.5, -1.1, 0.7],
            false,
            { render: true, pick: false },
            { debugShowShadowVolume: true },
          ),
        ];
        context.destroy();
        return { cases };
      })();

      // ======================================================================
      // 10. GroundPolylinePrimitive
      // ======================================================================
      const groundPolylinePrimitive = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(32, 32, [7000000, 0, 0], [-1, 0, 0], [0, 0, 1]);

        const runPolyline = (name, opts) => {
          const geom = new ref.GroundPolylineGeometry({
            positions: ref.Cartesian3.fromDegreesArray([-112.134, 36.055, -112.088, 36.098]),
            width: 5.0,
          });
          const inst = new ref.GeometryInstance({
            geometry: geom,
            id: name,
            attributes: opts.color
              ? { color: ref.ColorGeometryInstanceAttribute.fromColor(new ref.Color(0, 1, 0, 0.7)) }
              : {},
          });
          const appearance = opts.color
            ? new ref.PolylineColorAppearance()
            : new ref.PolylineMaterialAppearance();
          const gpp = new ref.GroundPolylinePrimitive({
            geometryInstances: inst,
            appearance,
            asynchronous: false,
            allowPicking: opts.allowPicking ?? true,
            classificationType: opts.classificationType ?? ref.ClassificationType.BOTH,
            debugShowShadowVolume: opts.debugShowShadowVolume ?? false,
          });

          const camCV = makeCamera(32, 32, [0, 0, 1000000], [0, 0, -1], [0, 1, 0], 1.0, 10000.0, ref.SceneMode.COLUMBUS_VIEW);
          const modeResults = {};
          for (const [mName, mode, render, pick] of [
            ['scene3d', ref.SceneMode.SCENE3D, true, false],
            ['columbus', ref.SceneMode.COLUMBUS_VIEW, true, false],
            ['morphing', ref.SceneMode.MORPHING, true, false],
            ['pick3d', ref.SceneMode.SCENE3D, false, true],
            ['pickMorph', ref.SceneMode.MORPHING, false, true],
          ]) {
            const activeCam = mode === ref.SceneMode.COLUMBUS_VIEW ? camCV : cam;
            const fs = makeFrameState(context, activeCam, { mode, render, pick });
            gpp.update(fs);
            for (const cb of fs.afterRender) cb();
            modeResults[mName] = fs.commandList.map(snapCmd);
          }

          const out = { name, ready: gpp.ready, modeResults, glError: gl.getError() };
          gpp.destroy();
          return out;
        };

        const cases = [
          runPolyline('material-both', { color: false }),
          runPolyline('color-terrain-debug', {
            color: true,
            classificationType: ref.ClassificationType.TERRAIN,
            debugShowShadowVolume: true,
          }),
          runPolyline('tiles-no-pick', {
            color: false,
            classificationType: ref.ClassificationType.CESIUM_3D_TILE,
            allowPicking: false,
          }),
        ];
        context.destroy();
        return { cases };
      })();

      // ======================================================================
      // 11. ShadowMap
      // ======================================================================
      const shadowMap = (() => {
        const canvas = makeCanvas(64, 64);
        const context = new ref.Context(canvas, { webgl: attrs });
        const gl = context._gl;
        const cam = makeCamera(64, 64, [7000000, 0, 0], [-1, 0, 0], [0, 1, 0], 1.0, 5000.0);
        const lightCam = makeCamera(64, 64, [7000000, 0, 0], [-1, 0, 0], [0, 1, 0], 1.0, 10000.0);

        const vb = ref.Buffer.createVertexBuffer({
          context,
          typedArray: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
          usage: ref.BufferUsage.STATIC_DRAW,
        });
        const va = new ref.VertexArray({
          context,
          attributes: [
            {
              index: 0,
              vertexBuffer: vb,
              componentsPerAttribute: 3,
              componentDatatype: ref.ComponentDatatype.FLOAT,
            },
          ],
        });
        const baseSp = ref.ShaderProgram.fromCache({
          context,
          vertexShaderSource: new ref.ShaderSource({
            sources: ['in vec3 position;\nout vec3 v_normalEC;\nvoid main() { v_normalEC = vec3(0.0, 0.0, 1.0); gl_Position = czm_modelViewProjection * vec4(position, 1.0); }'],
          }),
          fragmentShaderSource: new ref.ShaderSource({
            sources: ['in vec3 v_normalEC;\nvoid main() { out_FragColor = vec4(v_normalEC * 0.5 + 0.5, 1.0); }'],
          }),
          attributeLocations: { position: 0 },
        });

        const runShadow = (name, smOpts) => {
          const sm = new ref.ShadowMap({
            context,
            lightCamera: lightCam,
            size: 64,
            ...smOpts,
          });
          sm.debugShow = true;
          sm.debugFreezeFrame = true;
          const fs = makeFrameState(context, cam, { shadowNear: 1.0, shadowFar: 5000.0, closestObjectSize: 1000.0 });
          sm.update(fs);
          sm.updatePass(context, 0);

          const baseCmd = new ref.DrawCommand({
            vertexArray: va,
            shaderProgram: baseSp,
            renderState: ref.RenderState.fromCache({ cull: { enabled: true } }),
            pass: ref.Pass.OPAQUE,
            castShadows: true,
            receiveShadows: true,
            uniformMap: {},
          });
          const castRes = ref.ShadowMap.createCastDerivedCommand([sm], baseCmd, true, context);
          const recvRes = ref.ShadowMap.createReceiveDerivedCommand([sm], baseCmd, true, context);

          // Also derive translucent cast command
          const transCmd = ref.DrawCommand.shallowClone(baseCmd);
          transCmd.pass = ref.Pass.TRANSLUCENT;
          const castTransRes = ref.ShadowMap.createCastDerivedCommand([sm], transCmd, true, context);

          // Execute shadow cast command into pass 0 framebuffer on SwiftShader WebGL2
          context.uniformState.updateCamera(sm._passes[0].camera);
          castRes.castCommands[0].execute(context, sm._passes[0].passState);

          const evalUniforms = (uMap) => ({
            shadowMap_matrix: m4(uMap.shadowMap_matrix()),
            shadowMap_cascadeSplits: uMap.shadowMap_cascadeSplits().map(c4),
            shadowMap_cascadeMatrices: uMap.shadowMap_cascadeMatrices().map(m4),
            shadowMap_lightDirectionEC: c3(uMap.shadowMap_lightDirectionEC()),
            shadowMap_lightPositionEC: c4(uMap.shadowMap_lightPositionEC()),
            shadowMap_cascadeDistances: c4(uMap.shadowMap_cascadeDistances()),
            shadowMap_texelSizeDepthBiasAndNormalShadingSmooth: c4(
              uMap.shadowMap_texelSizeDepthBiasAndNormalShadingSmooth(),
            ),
            shadowMap_normalOffsetScaleDistanceMaxDistanceAndDarkness: c4(
              uMap.shadowMap_normalOffsetScaleDistanceMaxDistanceAndDarkness(),
            ),
          });

          const tex = sm._shadowMapTexture;
          const out = {
            name,
            usesDepthTexture: sm._usesDepthTexture,
            isPointLight: sm.isPointLight,
            isSpotLight: sm._isSpotLight,
            passCount: sm.passes.length,
            textureSize: c2(sm._textureSize),
            texture: {
              width: tex.width,
              height: tex.height,
              pixelFormat: tex.pixelFormat,
              pixelDatatype: tex.pixelDatatype,
              isCubeMap: tex instanceof ref.CubeMap,
            },
            fboStatus: sm.passes[0].framebuffer.status,
            viewports: sm.passes.map((p) => rect(p.passState.viewport)),
            uniforms: evalUniforms(recvRes.receiveCommand.uniformMap),
            castCmd: {
              vsDefines: cleanDefines(castRes.castCommands[0].shaderProgram.vertexShaderSource.defines),
              fsDefines: cleanDefines(castRes.castCommands[0].shaderProgram.fragmentShaderSource.defines),
              fsSource: castRes.castCommands[0].shaderProgram.fragmentShaderSource.sources.join('\n'),
              renderState: snapRS(castRes.castCommands[0].renderState),
            },
            castTranslucentFs: castTransRes.castCommands[0].shaderProgram.fragmentShaderSource.sources.join('\n'),
            receiveCmd: {
              vsDefines: cleanDefines(recvRes.receiveCommand.shaderProgram.vertexShaderSource.defines),
              fsDefines: cleanDefines(recvRes.receiveCommand.shaderProgram.fragmentShaderSource.defines),
              fsSource: recvRes.receiveCommand.shaderProgram.fragmentShaderSource.sources.join('\n'),
            },
            debugShadowView: sm._debugShadowViewCommand
              ? {
                  pass: sm._debugShadowViewCommand.pass,
                  viewport: rect(sm._debugShadowViewCommand.renderState.viewport),
                  fsHasCube: sm._debugShadowViewCommand.shaderProgram.fragmentShaderSource.sources
                    .join('\n')
                    .includes('shadowMap_textureCube'),
                }
              : null,
            debugCommandCount: fs.commandList.length,
            glError: gl.getError(),
          };
          sm.destroy();
          return out;
        };

        const cases = [
          runShadow('cascaded-4-soft', { numberOfCascades: 4, softShadows: true }),
          runShadow('point-light', { isPointLight: true, pointLightRadius: 250 }),
          runShadow('spot-light', { cascadesEnabled: false }),
        ];
        va.destroy();
        context.destroy();
        return {
          sceneCamera: {
            view: m4(cam.viewMatrix),
            inverseView: m4(cam.inverseViewMatrix),
            position: c3(cam.positionWC),
            direction: c3(cam.directionWC),
            up: c3(cam.upWC),
            right: c3(cam.rightWC),
            near: cam.frustum.near,
            far: cam.frustum.far,
            fov: cam.frustum.fov,
            aspectRatio: cam.frustum.aspectRatio,
          },
          lightCamera: {
            view: m4(lightCam.viewMatrix),
            inverseView: m4(lightCam.inverseViewMatrix),
            position: c3(lightCam.positionWC),
            direction: c3(lightCam.directionWC),
            up: c3(lightCam.upWC),
            right: c3(lightCam.rightWC),
            near: lightCam.frustum.near,
            far: lightCam.frustum.far,
            fov: lightCam.frustum.fov,
            aspectRatio: lightCam.frustum.aspectRatio,
          },
          cases,
        };
      })();

      // ======================================================================
      // 12. GoogleStreetViewCubeMapPanoramaProvider
      // ======================================================================
      const googleStreetView = await (async () => {
        const provider = await ref.GoogleStreetViewCubeMapPanoramaProvider.fromUrl({
          key: 'TEST_KEY',
          url: 'https://maps.googleapis.com/maps/api/streetview',
          tileSize: 512,
        });

        const requestedJsonUrls = [];
        const requestedImageUrls = [];
        const origFetchJson = ref.Resource.prototype.fetchJson;
        const origFetchImage = ref.Resource.fetchImage;

        let mockJsonResponse = { status: 'OK', pano_id: 'PANO_ABC', location: { lat: 37.7749, lng: -122.4194 } };
        ref.Resource.prototype.fetchJson = async function () {
          requestedJsonUrls.push(this.url);
          return mockJsonResponse;
        };
        ref.Resource.fetchImage = async function (opts) {
          requestedImageUrls.push(typeof opts === 'string' ? opts : opts.url);
          return new ImageData(4, 4);
        };

        try {
          const carto = ref.Cartographic.fromDegrees(-122.4194, 37.7749, 0.0);
          const nearestDefault = await provider.getNearestPanoId(carto);
          const nearestCustomRadius = await provider.getNearestPanoId(carto, 120);
          const metaOk = await provider.getPanoIdMetadata('PANO_ABC');

          mockJsonResponse = { status: 'ZERO_RESULTS' };
          let nearestErr = null;
          try {
            await provider.getNearestPanoId(carto);
          } catch (e) {
            nearestErr = e.message;
          }

          mockJsonResponse = { status: 'NOT_FOUND' };
          let metaErr = null;
          try {
            await provider.getPanoIdMetadata('BAD_PANO');
          } catch (e) {
            metaErr = e.message;
          }

          // loadPanorama with explicit panoId + signature + tileSize
          const panoExplicit = await provider.loadPanorama({
            cartographic: carto,
            panoId: 'PANO_SIGNED',
            tileSize: 256,
            signature: 'SIG123',
          });
          const explicitFaceUrls = requestedImageUrls.splice(0, requestedImageUrls.length);

          // loadPanorama without panoId -> resolves via getNearestPanoId
          mockJsonResponse = { status: 'OK', pano_id: 'PANO_LOOKUP', location: { lat: 37.7749, lng: -122.4194 } };
          const panoLookup = await provider.loadPanorama({ cartographic: carto });
          const lookupFaceUrls = requestedImageUrls.splice(0, requestedImageUrls.length);

          let missingKeyErr = null;
          try {
            await ref.GoogleStreetViewCubeMapPanoramaProvider.fromUrl({});
          } catch (e) {
            missingKeyErr = e.message;
          }

          return {
            cartographicDeg: [-122.4194, 37.7749, 0.0],
            requestedJsonUrls,
            nearestDefault,
            nearestCustomRadius,
            metaOk,
            nearestErr,
            metaErr,
            missingKeyErr,
            explicitFaceUrls,
            explicitTransform: m3(panoExplicit.transform),
            explicitCredit: panoExplicit.credit.html,
            lookupFaceUrls,
            lookupTransform: m3(panoLookup.transform),
            lookupCredit: panoLookup.credit.html,
          };
        } finally {
          ref.Resource.prototype.fetchJson = origFetchJson;
          ref.Resource.fetchImage = origFetchImage;
        }
      })();

      return {
        skyAtmosphere,
        sun,
        sunPostProcess,
        cubeMapPanorama,
        invertClassification,
        specularEnvironmentCubeMap,
        dynamicEnvironmentMapManager,
        classificationPrimitive,
        groundPrimitive,
        groundPolylinePrimitive,
        shadowMap,
        googleStreetView,
      };
    } catch (e) {
      return { __error: String(e), stack: e?.stack ?? '' };
    }
  }, approxTerrainHeights);

  if (result?.__error) {
    throw new Error(`${result.__error}\n${result.stack}`);
  }

  writeFileSync(
    new URL('../../tests/data/scene-atmosphere-gpu.json', import.meta.url),
    JSON.stringify(result, null, 2),
  );
  console.log('wrote tests/data/scene-atmosphere-gpu.json');
} finally {
  if (browser) await browser.close();
  server.close();
}
