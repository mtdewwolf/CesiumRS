// SwiftShader WebGL2 reference captures from the pinned @cesium/engine 23.0.0
// for the WebGL pending-validation Scene modules:
// - VoxelBoundsCollection
// - BrdfLutGenerator
// - PickedMetadataInfo (+ DerivedCommand.createPickMetadataDerivedCommand & MetadataPicking.decodeMetadataValues)
// - TranslucentTileClassification
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-pending-gpu.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: [
    'VoxelBoundsCollection',
    'ClippingPlane',
    'BrdfLutGenerator',
    'PickedMetadataInfo',
    'DerivedCommand',
    'MetadataPicking',
    'MetadataClassProperty',
    'MetadataComponentType',
    'MetadataType',
    'TranslucentTileClassification',
    'StencilConstants',
    'StencilFunction',
    'BlendingState',
    'ClassificationType',
    'SceneMode',
    'ShadowMode',
    'Model/MetadataPickingPipelineStage',
    'Model/ModelAlphaOptions',
    'Model/ModelDrawCommands',
    'Model/ModelDrawCommand',
    'Model/ClassificationModelDrawCommand',
  ],
  Renderer: [
    'Buffer',
    'BufferUsage',
    'Context',
    'ContextLimits',
    'Framebuffer',
    'Pass',
    'PassState',
    'PixelDatatype',
    'RenderState',
    'Sampler',
    'ShaderBuilder',
    'ShaderProgram',
    'ShaderSource',
    'Texture',
    'ClearCommand',
    'DrawCommand',
  ],
  Core: [
    'BoundingRectangle',
    'BoundingSphere',
    'Cartesian2',
    'Cartesian3',
    'Cartesian4',
    'Color',
    'ComponentDatatype',
    'IndexDatatype',
    'Matrix4',
    'PixelFormat',
    'Plane',
    'PrimitiveType',
    'Quaternion',
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

const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') {
    res.setHeader('Content-Type', 'text/javascript');
    res.end(bundle.outputFiles[0].contents);
    return;
  }
  res.end('<!doctype html><title>Pending Scene GPU validations</title>');
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

  const result = await page.evaluate(async () => {
   try {
    const ref = await import('/reference.mjs');
    const fnv = (bytes) => {
      let h = 0xcbf29ce484222325n;
      for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
      return h.toString(16);
    };
    const toBase64 = (bytes) => {
      let s = '';
      for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
      return btoa(s);
    };
    const makeCanvas = (w, h) => {
      const c = document.createElement('canvas');
      c.width = w;
      c.height = h;
      return c;
    };
    const attrs = { alpha: true, depth: true, stencil: true, antialias: false, preserveDrawingBuffer: true };

    // --- 1. VoxelBoundsCollection on SwiftShader WebGL2 ---------------------
    const voxelBoundsCollection = (() => {
      const canvas = makeCanvas(8, 8);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;
      const uploads = [];
      const origTexSub = gl.texSubImage2D.bind(gl);
      gl.texSubImage2D = function (...args) {
        const last = args[args.length - 1];
        if (last && ArrayBuffer.isView(last)) {
          const bytes = new Uint8Array(last.buffer, last.byteOffset, last.byteLength);
          uploads.push({
            width: args[4],
            height: args[5],
            format: args[6],
            type: args[7],
            byteLength: bytes.byteLength,
            hash: fnv(bytes),
            floats: Array.from(new Float32Array(last.buffer, last.byteOffset, last.byteLength / 4)),
          });
        }
        return origTexSub(...args);
      };

      const p0 = new ref.ClippingPlane(new ref.Cartesian3(1, 0, 0), 2.5);
      const p1 = new ref.ClippingPlane(new ref.Cartesian3(0, 1, 0), -4.0);
      const initialModel = ref.Matrix4.fromTranslation(new ref.Cartesian3(1, 2, 3));
      const events = [];
      const col = new ref.VoxelBoundsCollection({
        planes: [p0, p1],
        modelMatrix: initialModel,
        unionClippingRegions: true,
      });
      col.planeAdded.addEventListener((plane, index) => events.push({ kind: 'added', index, distance: plane.distance }));
      col.planeRemoved.addEventListener((plane, index) => events.push({ kind: 'removed', index, distance: plane.distance }));

      const transformA = ref.Matrix4.IDENTITY;
      const transformB = ref.Matrix4.fromTranslationQuaternionRotationScale(
        new ref.Cartesian3(10, -20, 5),
        ref.Quaternion.fromAxisAngle(ref.Cartesian3.normalize(new ref.Cartesian3(1, 2, 3), new ref.Cartesian3()), 0.75),
        new ref.Cartesian3(2, 0.5, 4),
      );

      const snapshot = (label) => {
        const res = col.length > 0 ? ref.VoxelBoundsCollection.getTextureResolution(col, context, new ref.Cartesian2()) : null;
        const tex = col.texture;
        // Read back the float texture through an FBO to verify GPU storage matches CPU upload
        let gpuReadbackHash = null;
        let gpuReadbackFloats = null;
        if (tex) {
          const fb = new ref.Framebuffer({ context, colorTextures: [tex], destroyAttachments: false });
          const out = new Float32Array(tex.width * tex.height * 4);
          gl.bindFramebuffer(gl.FRAMEBUFFER, fb._framebuffer);
          gl.readPixels(0, 0, tex.width, tex.height, gl.RGBA, gl.FLOAT, out);
          gl.bindFramebuffer(gl.FRAMEBUFFER, null);
          gpuReadbackHash = fnv(new Uint8Array(out.buffer));
          gpuReadbackFloats = Array.from(out);
          fb.destroy();
        }
        const ev = events.splice(0, events.length);
        const up = uploads.splice(0, uploads.length);
        return {
          label,
          length: col.length,
          unionClippingRegions: col.unionClippingRegions,
          clippingPlanesState: col.clippingPlanesState,
          indices: Array.from({ length: col.length }, (_, i) => col.get(i).index),
          textureResolution: res ? [res.x, res.y] : null,
          texture: tex
            ? {
                width: tex.width,
                height: tex.height,
                pixelFormat: tex.pixelFormat,
                pixelDatatype: tex.pixelDatatype,
                flipY: tex.flipY,
              }
            : null,
          events: ev,
          uploads: up,
          gpuReadbackHash,
          gpuReadbackFloats,
          glError: gl.getError(),
        };
      };

      const steps = [];
      steps.push(snapshot('ctor'));
      col.update({ context }, transformA);
      steps.push(snapshot('update-initial'));
      col.unionClippingRegions = false;
      col.update({ context }, transformB);
      steps.push(snapshot('update-transformed'));
      p1.distance = 7.25;
      p1.normal = ref.Cartesian3.normalize(new ref.Cartesian3(0, 3, 4), new ref.Cartesian3());
      col.update({ context }, transformB);
      steps.push(snapshot('mutate-plane'));
      // Grow past capacity (2x2 = 4 pixels): add 4 more planes -> 6 planes -> 6x2 = 12 pixels
      const extra = [
        new ref.ClippingPlane(new ref.Cartesian3(0, 0, 1), 1.0),
        new ref.ClippingPlane(new ref.Cartesian3(-1, 0, 0), 3.0),
        new ref.ClippingPlane(new ref.Cartesian3(0, -1, 0), 5.0),
        new ref.ClippingPlane(new ref.Cartesian3(0, 0, -1), 6.0),
      ];
      for (const p of extra) col.add(p);
      col.update({ context }, transformA);
      steps.push(snapshot('grow-to-6'));
      // Remove 2 planes -> 4 planes remaining. 4 >= 0.25 * 12 (3), so NO texture resize!
      const removedP0 = col.remove(new ref.Plane(new ref.Cartesian3(1, 0, 0), 2.5));
      const removedMissing = col.remove(new ref.Plane(new ref.Cartesian3(1, 0, 0), 999));
      const removedExtra1 = col.remove(extra[1]);
      col.update({ context }, transformA);
      steps.push({ ...snapshot('remove-no-resize'), removedP0, removedMissing, removedExtra1, p0IndexAfterRemove: p0.index });
      // Remove 2 more -> 2 planes remaining. 2 < 0.25 * 12 (3), so SHRINK to 2x2!
      col.remove(extra[0]);
      col.remove(extra[2]);
      col.update({ context }, transformA);
      steps.push(snapshot('shrink-to-2'));
      // removeAll -> update destroys texture
      col.removeAll();
      col.update({ context }, transformA);
      steps.push(snapshot('remove-all'));
      // Re-add 1 plane -> allocates 1x2
      col.add(new ref.ClippingPlane(new ref.Cartesian3(0, 0, 1), -1.5));
      col.update({ context }, transformA);
      steps.push(snapshot('re-add-1'));
      col.destroy();
      const destroyed = col.isDestroyed();
      context.destroy();
      return {
        maxTextureSize: ref.ContextLimits.maximumTextureSize,
        initialModel: ref.Matrix4.pack(initialModel, []),
        transformB: ref.Matrix4.pack(transformB, []),
        steps,
        destroyed,
      };
    })();

    // --- 2. BrdfLutGenerator on SwiftShader WebGL2 --------------------------
    const brdfLutGenerator = (() => {
      const canvas = makeCanvas(256, 256);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;
      let draws = 0;
      const origDraw = gl.drawElements.bind(gl);
      gl.drawElements = function (...a) {
        draws++;
        return origDraw(...a);
      };
      const gen = new ref.BrdfLutGenerator();
      const beforeTexture = gen.colorTexture ?? null;
      gen.update({ context });
      const tex1 = gen.colorTexture;
      const drawsAfterFirst = draws;
      // Second update is idempotent
      gen.update({ context });
      const sameTextureOnSecond = gen.colorTexture === tex1;
      const drawsAfterSecond = draws;
      const fb = new ref.Framebuffer({ context, colorTextures: [tex1], destroyAttachments: false });
      const pixels = context.readPixels({ framebuffer: fb, width: 256, height: 256 });
      fb.destroy();
      const sampleIndices = [0, 4, 255 * 4, (128 * 256 + 128) * 4, (255 * 256 + 255) * 4];
      const samples = sampleIndices.map((idx) => [idx, pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3]]);
      const out = {
        beforeTexture,
        width: tex1.width,
        height: tex1.height,
        pixelFormat: tex1.pixelFormat,
        pixelDatatype: tex1.pixelDatatype,
        sampler: {
          minificationFilter: tex1.sampler.minificationFilter,
          magnificationFilter: tex1.sampler.magnificationFilter,
          wrapS: tex1.sampler.wrapS,
          wrapT: tex1.sampler.wrapT,
        },
        viewport: {
          x: gen._drawCommand.renderState.viewport.x,
          y: gen._drawCommand.renderState.viewport.y,
          width: gen._drawCommand.renderState.viewport.width,
          height: gen._drawCommand.renderState.viewport.height,
        },
        drawsAfterFirst,
        sameTextureOnSecond,
        drawsAfterSecond,
        byteLength: pixels.length,
        hash: fnv(pixels),
        samples,
        glError: gl.getError(),
      };
      gen.destroy();
      out.isDestroyedAfter = gen.isDestroyed();
      context.destroy();
      return out;
    })();

    // --- 3. PickedMetadataInfo + DerivedCommand + MetadataPicking -----------
    const pickedMetadataInfo = (() => {
      const canvas = makeCanvas(4, 4);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;
      const scene = { picking: { pickRenderStateCache: {} } };

      const propSpecs = [
        {
          name: 'scalar-uint8-unnorm',
          schemaId: 'schemaA',
          className: 'Building',
          propertyName: 'height',
          classProp: { type: 'SCALAR', componentType: 'UINT8', normalized: false },
          hasValueTransform: false,
          glslLiteral: 'uint(180)',
        },
        {
          name: 'scalar-uint8-norm-no-schema',
          schemaId: undefined,
          className: 'Building',
          propertyName: 'ratio',
          classProp: { type: 'SCALAR', componentType: 'UINT8', normalized: true },
          hasValueTransform: false,
          glslLiteral: 'float(102.0 / 255.0)',
        },
        {
          name: 'scalar-uint8-transform',
          schemaId: 'schemaT',
          className: 'Building',
          propertyName: 'temp',
          classProp: { type: 'SCALAR', componentType: 'UINT8', normalized: false, offset: 10, scale: 2 },
          hasValueTransform: true,
          offset: 10,
          scale: 2,
          glslLiteral: 'uint(110)',
        },
        {
          name: 'vec2-uint8-unnorm',
          schemaId: 'schemaV',
          className: 'Plot',
          propertyName: 'xy',
          classProp: { type: 'VEC2', componentType: 'UINT8', normalized: false },
          hasValueTransform: false,
          glslLiteral: 'uvec2(64u, 192u)',
        },
        {
          name: 'vec3-uint8-norm-transform',
          schemaId: 'schemaV3',
          className: 'Plot',
          propertyName: 'rgb',
          classProp: { type: 'VEC3', componentType: 'UINT8', normalized: true, offset: [0.1, 0.2, 0.3], scale: [0.5, 0.5, 0.5] },
          hasValueTransform: true,
          offset: new ref.Cartesian3(0.1, 0.2, 0.3),
          scale: new ref.Cartesian3(0.5, 0.5, 0.5),
          glslLiteral: 'vec3(0.1 + 0.5 * (51.0/255.0), 0.2 + 0.5 * (102.0/255.0), 0.3 + 0.5 * (204.0/255.0))',
        },
        {
          name: 'vec4-uint8-unnorm',
          schemaId: 'schemaV4',
          className: 'Plot',
          propertyName: 'rgba',
          classProp: { type: 'VEC4', componentType: 'UINT8', normalized: false },
          hasValueTransform: false,
          glslLiteral: 'uvec4(10u, 20u, 30u, 250u)',
        },
        {
          name: 'scalar-array3-uint8',
          schemaId: 'schemaArr',
          className: 'Plot',
          propertyName: 'arr',
          classProp: { type: 'SCALAR', componentType: 'UINT8', array: true, count: 3, normalized: false },
          hasValueTransform: false,
          glslLiteral: null,
        },
      ];

      const cases = propSpecs.map((spec) => {
        const classProperty = ref.MetadataClassProperty.fromJson({
          id: spec.propertyName,
          property: spec.classProp,
        });
        const metadataProperty = {
          hasValueTransform: spec.hasValueTransform,
          offset: spec.offset ?? classProperty.offset,
          scale: spec.scale ?? classProperty.scale,
        };
        const info = new ref.PickedMetadataInfo(
          spec.schemaId,
          spec.className,
          spec.propertyName,
          classProperty,
          metadataProperty,
        );

        const baseDefines = [
          'EXISTING_DEFINE',
          `${ref.MetadataPickingPipelineStage.METADATA_PICKING_VALUE_TYPE} float`,
          `${ref.MetadataPickingPipelineStage.METADATA_PICKING_VALUE_STRING} 0.0`,
          `${ref.MetadataPickingPipelineStage.METADATA_PICKING_VALUE_COMPONENT_X} 0.0`,
          `${ref.MetadataPickingPipelineStage.METADATA_PICKING_VALUE_COMPONENT_Y} 0.0`,
          `${ref.MetadataPickingPipelineStage.METADATA_PICKING_VALUE_COMPONENT_Z} 0.0`,
          `${ref.MetadataPickingPipelineStage.METADATA_PICKING_VALUE_COMPONENT_W} 0.0`,
        ];

        const fsBody = spec.glslLiteral
          ? `struct Metadata { METADATA_PICKING_VALUE_TYPE ${spec.propertyName}; };
void main() {
  Metadata metadata;
  metadata.${spec.propertyName} = ${spec.glslLiteral};
  #ifdef METADATA_PICKING_ENABLED
  METADATA_PICKING_VALUE_TYPE value = METADATA_PICKING_VALUE_STRING;
  out_FragColor = vec4(
    METADATA_PICKING_VALUE_COMPONENT_X,
    METADATA_PICKING_VALUE_COMPONENT_Y,
    METADATA_PICKING_VALUE_COMPONENT_Z,
    METADATA_PICKING_VALUE_COMPONENT_W
  );
  #else
  out_FragColor = vec4(1.0);
  #endif
}`
          : 'void main() { out_FragColor = vec4(1.0); }';

        const baseCommand = context.createViewportQuadCommand(
          new ref.ShaderSource({ defines: baseDefines, sources: [fsBody] }),
          { renderState: ref.RenderState.fromCache({ viewport: new ref.BoundingRectangle(0, 0, 4, 4) }) },
        );
        baseCommand.pickedMetadataInfo = info;

        const derived = ref.DerivedCommand.createPickMetadataDerivedCommand(scene, baseCommand, context);
        const derivedDefines = [...derived.pickMetadataCommand.shaderProgram.fragmentShaderSource.defines];

        let pixel = null;
        let decoded = null;
        if (spec.glslLiteral) {
          const pass = new ref.PassState(context);
          pass.viewport = new ref.BoundingRectangle(0, 0, 4, 4);
          new ref.ClearCommand({ color: ref.Color.TRANSPARENT }).execute(context, pass);
          derived.pickMetadataCommand.execute(context, pass);
          const raw = context.readPixels({ width: 4, height: 4 });
          pixel = [raw[0], raw[1], raw[2], raw[3]];
          const val = ref.MetadataPicking.decodeMetadataValues(classProperty, metadataProperty, new Uint8Array(pixel));
          decoded = val instanceof ref.Cartesian2
            ? [val.x, val.y]
            : val instanceof ref.Cartesian3
              ? [val.x, val.y, val.z]
              : val instanceof ref.Cartesian4
                ? [val.x, val.y, val.z, val.w]
                : val;
        }
        return {
          name: spec.name,
          schemaId: info.schemaId ?? null,
          className: info.className,
          propertyName: info.propertyName,
          classProp: spec.classProp,
          hasValueTransform: spec.hasValueTransform,
          offset: spec.classProp.offset ?? null,
          scale: spec.classProp.scale ?? null,
          cacheKey: `pickMetadata-${info.schemaId}-${info.className}-${info.propertyName}`,
          derivedDefines,
          pixel,
          decoded,
          glError: gl.getError(),
        };
      });

      context.destroy();
      return cases;
    })();

    // --- 4. TranslucentTileClassification on SwiftShader WebGL2 -------------
    const translucentTileClassification = (() => {
      const canvas = makeCanvas(8, 8);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;
      const ttc = new ref.TranslucentTileClassification(context);

      const makeOpaqueDepth = (w, h) =>
        new ref.Texture({
          context,
          width: w,
          height: h,
          pixelFormat: ref.PixelFormat.DEPTH_STENCIL,
          pixelDatatype: ref.PixelDatatype.UNSIGNED_INT_24_8,
          sampler: ref.Sampler.NEAREST,
        });

      const opaque8 = makeOpaqueDepth(8, 8);
      const initialGlobeDepth = new ref.Texture({
        context,
        width: 8,
        height: 8,
        pixelFormat: ref.PixelFormat.RGBA,
        pixelDatatype: ref.PixelDatatype.UNSIGNED_BYTE,
      });
      context.uniformState.globeDepthTexture = initialGlobeDepth;

      const makePass = (vp) => {
        const p = new ref.PassState(context);
        p.viewport = new ref.BoundingRectangle(...vp);
        return p;
      };

      const fakeScene = (pick) => ({
        _context: context,
        context,
        frameState: { useLogDepth: false, passes: { pick } },
      });

      const depthDrawCmd = context.createViewportQuadCommand('void main() { out_FragColor = vec4(1.0); }', {
        renderState: ref.RenderState.fromCache({ viewport: new ref.BoundingRectangle(0, 0, 8, 8), depthTest: { enabled: true } }),
      });
      const flaggedCmd = {
        depthForTranslucentClassification: true,
        derivedCommands: { depth: { depthOnlyCommand: depthDrawCmd } },
      };
      const unflaggedCmd = {
        depthForTranslucentClassification: false,
        derivedCommands: { depth: { depthOnlyCommand: depthDrawCmd } },
      };

      const classifyDrawCmd = context.createViewportQuadCommand('void main() { out_FragColor = vec4(0.25, 0.5, 0.75, 0.5); }', {
        renderState: ref.RenderState.fromCache({ viewport: new ref.BoundingRectangle(0, 0, 8, 8) }),
      });
      const frustumCommands = {
        commands: { [ref.Pass.CESIUM_3D_TILE_CLASSIFICATION]: [classifyDrawCmd] },
        indices: { [ref.Pass.CESIUM_3D_TILE_CLASSIFICATION]: 1 },
      };

      const log = [];
      const execCmd = (cmd, sc, ps) => {
        log.push({
          pass: context.uniformState.pass,
          globeDepthIsPacked: context.uniformState.globeDepthTexture === ttc._packFBO.getColorTexture(),
          fbIsClassification: ps.framebuffer === ttc._drawClassificationFBO.framebuffer,
        });
        cmd.execute(context, ps);
      };

      const rsSnap = (rs) => rs ? ({
        viewport: [rs.viewport.x, rs.viewport.y, rs.viewport.width, rs.viewport.height],
        scissorEnabled: rs.scissorTest.enabled,
        scissorRect: [rs.scissorTest.rectangle.x, rs.scissorTest.rectangle.y, rs.scissorTest.rectangle.width, rs.scissorTest.rectangle.height],
        stencilEnabled: rs.stencilTest.enabled,
        stencilFrontFunc: rs.stencilTest.frontFunction,
        stencilRef: rs.stencilTest.reference,
        blendingEnabled: rs.blending.enabled,
        blendingSrcRgb: rs.blending.functionSourceRgb,
        blendingDstRgb: rs.blending.functionDestinationRgb,
      }) : null;

      // Step 1: Unflagged command does nothing
      const passFull = makePass([0, 0, 8, 8]);
      ttc.executeTranslucentCommands(fakeScene(false), execCmd, passFull, [unflaggedCmd], opaque8);
      const afterUnflagged = {
        supported: ttc.isSupported(),
        hasTranslucentDepth: ttc.hasTranslucentDepth,
        frustumsDrawn: ttc._frustumsDrawn,
        hasDepthTex: !!ttc._translucentDepthStencilTexture,
      };

      // Step 2: Single frustum (render pass, full viewport -> scissor disabled)
      ttc.executeTranslucentCommands(fakeScene(false), execCmd, passFull, [flaggedCmd], opaque8);
      const afterDepth1 = {
        hasTranslucentDepth: ttc.hasTranslucentDepth,
        frustumsDrawn: ttc._frustumsDrawn,
        depthTex: {
          width: ttc._translucentDepthStencilTexture.width,
          height: ttc._translucentDepthStencilTexture.height,
          pixelFormat: ttc._translucentDepthStencilTexture.pixelFormat,
          pixelDatatype: ttc._translucentDepthStencilTexture.pixelDatatype,
        },
        rsDepth: rsSnap(ttc._rsDepth),
        rsAccumulate: rsSnap(ttc._rsAccumulate),
        rsComp: rsSnap(ttc._rsComp),
        pickDefines: [...ttc._compositeCommand.derivedCommands.pick.shaderProgram.fragmentShaderSource.defines],
      };
      ttc.executeClassificationCommands(fakeScene(false), execCmd, passFull, frustumCommands);
      const globeDepthRestored1 = context.uniformState.globeDepthTexture === initialGlobeDepth;
      ttc.execute(fakeScene(false), passFull);
      const afterExec1 = {
        compositeUsedClassification: ttc._textureToComposite === ttc._drawClassificationFBO.getColorTexture(),
        hasTranslucentDepth: ttc.hasTranslucentDepth,
        frustumsDrawn: ttc._frustumsDrawn,
        globeDepthRestored1,
      };

      // Step 3: Multi-frustum (2 frustums, sub-viewport -> scissor enabled, pick pass)
      const passSub = makePass([1, 2, 4, 4]);
      ttc.executeTranslucentCommands(fakeScene(true), execCmd, passSub, [flaggedCmd], opaque8);
      ttc.executeClassificationCommands(fakeScene(true), execCmd, passSub, frustumCommands);
      ttc.executeTranslucentCommands(fakeScene(true), execCmd, passSub, [flaggedCmd], opaque8);
      ttc.executeClassificationCommands(fakeScene(true), execCmd, passSub, frustumCommands);
      const beforeExec2 = {
        frustumsDrawn: ttc._frustumsDrawn,
        rsDepthSub: rsSnap(ttc._rsDepth),
        rsAccumulateSub: rsSnap(ttc._rsAccumulate),
        rsCompSub: rsSnap(ttc._rsComp),
      };
      ttc.execute(fakeScene(true), passSub);
      const afterExec2 = {
        compositeUsedAccumulation: ttc._textureToComposite === ttc._accumulationFBO.getColorTexture(),
        hasTranslucentDepth: ttc.hasTranslucentDepth,
        frustumsDrawn: ttc._frustumsDrawn,
        globeDepthRestored2: context.uniformState.globeDepthTexture === initialGlobeDepth,
      };

      // Step 4: Resize opaque depth to 12x6
      const opaque12 = makeOpaqueDepth(12, 6);
      const pass12 = makePass([0, 0, 12, 6]);
      ttc.executeTranslucentCommands(fakeScene(false), execCmd, pass12, [flaggedCmd], opaque12);
      const afterResize = {
        depthDims: [ttc._translucentDepthStencilTexture.width, ttc._translucentDepthStencilTexture.height],
        packedDims: [ttc._packFBO.getColorTexture().width, ttc._packFBO.getColorTexture().height],
        glError: gl.getError(),
      };

      ttc.destroy();
      const destroyed = ttc.isDestroyed();
      opaque8.destroy();
      opaque12.destroy();
      initialGlobeDepth.destroy();
      context.destroy();
      return {
        afterUnflagged,
        afterDepth1,
        afterExec1,
        beforeExec2,
        afterExec2,
        afterResize,
        execLog: log,
        destroyed,
      };
    })();

    // --- 5. ModelDrawCommands on SwiftShader WebGL2 -------------------------
    const modelDrawCommands = (() => {
      const canvas = makeCanvas(8, 8);
      const context = new ref.Context(canvas, { webgl: attrs });
      const gl = context._gl;

      const vb = ref.Buffer.createVertexBuffer({
        context,
        typedArray: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
        usage: ref.BufferUsage.STATIC_DRAW,
      });
      const ib = ref.Buffer.createIndexBuffer({
        context,
        typedArray: new Uint16Array([0, 1, 2]),
        usage: ref.BufferUsage.STATIC_DRAW,
        indexDatatype: ref.IndexDatatype.UNSIGNED_SHORT,
      });
      const wireIb = ref.Buffer.createIndexBuffer({
        context,
        typedArray: new Uint16Array([0, 1, 1, 2, 2, 0]),
        usage: ref.BufferUsage.STATIC_DRAW,
        indexDatatype: ref.IndexDatatype.UNSIGNED_SHORT,
      });

      const makeResources = (opts) => {
        const shaderBuilder = new ref.ShaderBuilder();
        shaderBuilder.addAttribute('vec3', 'a_position');
        const model = {
          _pipelineResources: [],
          classificationType: opts.classificationType,
          shadows: opts.shadows ?? ref.ShadowMode.ENABLED,
          cull: opts.cull ?? true,
          debugShowBoundingVolume: opts.debugShowBoundingVolume ?? false,
          allowPicking: opts.allowPicking ?? true,
          _projectTo2D: opts.projectTo2D ?? false,
          sceneGraph: {
            _computedModelMatrix: opts.modelMatrix ?? ref.Matrix4.IDENTITY,
            _computedModelMatrix2D: opts.modelMatrix ?? ref.Matrix4.IDENTITY,
            _components: { structuralMetadata: undefined },
          },
          _enableDebugWireframe: false,
          debugWireframe: false,
          _hasSilhouette: false,
          silhouetteColor: ref.Color.RED,
          silhouetteSize: 0,
        };
        const alphaOptions = new ref.ModelAlphaOptions();
        alphaOptions.pass = opts.pass ?? ref.Pass.OPAQUE;
        return {
          model,
          shaderBuilder,
          uniformMap: {},
          renderStateOptions: ref.RenderState.getState(ref.RenderState.fromCache({ cull: { enabled: true } })),
          alphaOptions,
          attributes: [
            {
              index: 0,
              vertexBuffer: vb,
              componentsPerAttribute: 3,
              componentDatatype: ref.ComponentDatatype.FLOAT,
              normalize: false,
              offsetInBytes: 0,
              strideInBytes: 12,
            },
          ],
          indices: opts.indices,
          wireframeIndexBuffer: opts.wireframeIndexBuffer,
          boundingSphere: new ref.BoundingSphere(new ref.Cartesian3(1, 2, 3), 5),
          primitiveType: opts.primitiveType ?? ref.PrimitiveType.TRIANGLES,
          count: opts.count ?? 3,
          instanceCount: opts.instanceCount,
          pickId: opts.pickId ?? 'czm_pickColor',
          runtimeNode: { computedTransform: opts.nodeTransform ?? ref.Matrix4.IDENTITY },
          runtimePrimitive: {
            boundingSphere2D: new ref.BoundingSphere(new ref.Cartesian3(10, 20, 30), 7),
            batchLengths: [3],
            batchOffsets: [0],
            primitive: { material: { doubleSided: false } },
          },
        };
      };

      const mirrorMatrix = ref.Matrix4.fromScale(new ref.Cartesian3(-1, 1, 1));
      const translateMatrix = ref.Matrix4.fromTranslation(new ref.Cartesian3(10, 20, 30));
      const nodeMatrix = ref.Matrix4.fromTranslation(new ref.Cartesian3(1, 2, 3));

      const specs = [
        {
          name: 'ordinary-3d',
          frame: { mode: ref.SceneMode.SCENE3D, scene3DOnly: false },
          res: {
            modelMatrix: translateMatrix,
            nodeTransform: nodeMatrix,
            indices: { buffer: ib },
            shadows: ref.ShadowMode.ENABLED,
            count: 3,
            instanceCount: 2,
          },
        },
        {
          name: 'mirrored-cull-front',
          frame: { mode: ref.SceneMode.SCENE3D, scene3DOnly: false },
          res: {
            modelMatrix: mirrorMatrix,
            indices: { buffer: ib },
            shadows: ref.ShadowMode.CAST_ONLY,
          },
        },
        {
          name: 'wireframe-index-override',
          frame: { mode: ref.SceneMode.SCENE3D, scene3DOnly: false },
          res: {
            indices: { buffer: ib },
            wireframeIndexBuffer: wireIb,
            primitiveType: ref.PrimitiveType.LINES,
            count: 6,
          },
        },
        {
          name: 'projected-2d',
          frame: { mode: ref.SceneMode.SCENE2D, scene3DOnly: false },
          res: {
            modelMatrix: translateMatrix,
            nodeTransform: nodeMatrix,
            projectTo2D: true,
            pass: ref.Pass.TRANSLUCENT,
          },
        },
        {
          name: 'classification',
          frame: { mode: ref.SceneMode.SCENE3D, scene3DOnly: false },
          res: {
            classificationType: ref.ClassificationType.CESIUM_3D_TILE,
            shadows: ref.ShadowMode.ENABLED,
            pickId: 'czm_pickColor',
            indices: { buffer: ib },
          },
        },
        {
          name: 'missing-indices-buffer-error',
          frame: { mode: ref.SceneMode.SCENE3D, scene3DOnly: false },
          res: {
            indices: { buffer: undefined },
          },
        },
      ];

      const cases = specs.map((spec) => {
        const resources = makeResources(spec.res);
        const frameState = { context, ...spec.frame };
        try {
          const built = ref.ModelDrawCommands.buildModelDrawCommand(resources, frameState);
          const isClassification = built instanceof ref.ClassificationModelDrawCommand;
          const cmd = built._command;
          const vsText = cmd.shaderProgram.vertexShaderSource.sources.join('\n');
          const fsText = cmd.shaderProgram.fragmentShaderSource.sources.join('\n');
          return {
            name: spec.name,
            error: null,
            isClassification,
            cullFace: cmd.renderState.cull.face,
            pass: cmd.pass,
            primitiveType: cmd.primitiveType,
            count: cmd.count,
            instanceCount: cmd.instanceCount ?? null,
            cull: cmd.cull,
            debugShowBoundingVolume: cmd.debugShowBoundingVolume,
            pickId: cmd.pickId ?? null,
            pickMetadataAllowed: cmd.pickMetadataAllowed,
            castShadows: cmd.castShadows,
            receiveShadows: cmd.receiveShadows,
            hasIndexBuffer: !!cmd.vertexArray.indexBuffer,
            usedWireframeIndexBuffer: cmd.vertexArray.indexBuffer === wireIb,
            modelMatrix: ref.Matrix4.pack(cmd.modelMatrix, []),
            boundingSphere: {
              center: [cmd.boundingVolume.center.x, cmd.boundingVolume.center.y, cmd.boundingVolume.center.z],
              radius: cmd.boundingVolume.radius,
            },
            hasModelVs: vsText.includes('modelMain'),
            hasModelFs: fsText.includes('modelMain'),
            glError: gl.getError(),
          };
        } catch (e) {
          return { name: spec.name, error: e.message };
        }
      });

      vb.destroy();
      ib.destroy();
      wireIb.destroy();
      context.destroy();
      return {
        translateMatrix: ref.Matrix4.pack(translateMatrix, []),
        mirrorMatrix: ref.Matrix4.pack(mirrorMatrix, []),
        nodeMatrix: ref.Matrix4.pack(nodeMatrix, []),
        cases,
      };
    })();

    return {
      cesium: '23.0.0',
      voxelBoundsCollection,
      brdfLutGenerator,
      pickedMetadataInfo,
      translucentTileClassification,
      modelDrawCommands,
    };
   } catch (e) {
     return { __error: e?.message ?? String(e), __stack: e?.stack ?? '' };
   }
  });
  if (result.__error) {
    throw new Error(`${result.__error}\n${result.__stack}`);
  }

  writeFileSync(new URL('../../tests/data/scene-pending-gpu.json', import.meta.url), JSON.stringify(result, null, 2) + '\n');
  console.log('Wrote tests/data/scene-pending-gpu.json');
} finally {
  if (browser) await browser.close();
  server.close();
}
