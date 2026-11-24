// SwiftShader WebGL2 reference captures from the pinned @cesium/engine 23.0.0
// for Cluster 5 (Point Clouds & Voxels):
// - Megatexture
// - VoxelPrimitive
// - Cesium3DTilesVoxelProvider
// - PointCloud (uncompressed, quantized, colors, normals, batchIds, binary batch-table styleable, Draco)
// - PointCloudEyeDomeLighting
// - TimeDynamicPointCloud
// Usage: CHROMIUM_PATH=/path/to/chrome node tools/cesium/generate-scene-pointcloud-voxel-gpu.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const decodeDracoFixture = JSON.parse(
  readFileSync(new URL('../../tests/fixtures/workers/decodeDraco.json', import.meta.url), 'utf8'),
);
const dracoWasmBytes = readFileSync(
  new URL('./node_modules/draco3d/draco_decoder.wasm', import.meta.url),
);

const modules = {
  Scene: [
    'Megatexture',
    'VoxelPrimitive',
    'VoxelContent',
    'VoxelMetadataOrder',
    'VoxelShapeType',
    'Cesium3DTilesVoxelProvider',
    'PointCloud',
    'PointCloudEyeDomeLighting',
    'PointCloudShading',
    'TimeDynamicPointCloud',
    'Cesium3DTileStyle',
    'ClippingPlane',
    'ClippingPlaneCollection',
    'DracoLoader',
    'MetadataComponentType',
    'MetadataType',
    'SceneMode',
    'ShadowMode',
    'SplitDirection',
    'Model/CustomShader',
    'Model/UniformType',
  ],
  Renderer: [
    'Buffer',
    'BufferUsage',
    'ClearCommand',
    'Context',
    'ContextLimits',
    'DrawCommand',
    'Pass',
    'PassState',
    'PixelDatatype',
    'RenderState',
    'Sampler',
    'ShaderProgram',
    'ShaderSource',
    'VertexArray',
  ],
  Core: [
    'BoundingRectangle',
    'BoundingSphere',
    'Cartesian2',
    'Cartesian3',
    'Cartesian4',
    'Clock',
    'ClockRange',
    'Color',
    'ComponentDatatype',
    'GeographicProjection',
    'JulianDate',
    'Matrix3',
    'Matrix4',
    'PerspectiveFrustum',
    'PixelFormat',
    'PrimitiveType',
    'Quaternion',
    'Resource',
    'TimeInterval',
    'TimeIntervalCollection',
  ],
  Workers: ['decodeDraco'],
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
  external: ['node:fs'],
  plugins: [{
    name: 'expose-draco-worker-callback',
    setup(build) {
      build.onLoad({filter: /[\\/]Workers[\\/]decodeDraco\.js$/}, ({path}) => ({
        contents: readFileSync(path, 'utf8').replace('export default createTaskProcessorWorker(decodeDraco);', 'export default decodeDraco;'),
      }));
    },
  }],
});

// Helper: build a .pnts binary buffer in Node
function buildPnts({
  positions,
  quantized = null,
  rgb = null,
  rgba = null,
  rgb565 = null,
  constantRgba = null,
  normals = null,
  normalsOct16p = null,
  batchIds = null,
  batchLength = null,
  rtcCenter = null,
  batchTableJson = null,
  batchTableBinary = null,
  draco = null,
}) {
  const ftJson = {};
  const ftParts = [];
  let ftOffset = 0;
  const pushFt = (bytes) => {
    const start = ftOffset;
    ftParts.push(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength));
    ftOffset += bytes.byteLength;
    return start;
  };
  const alignFt = (align) => {
    const rem = ftOffset % align;
    if (rem !== 0) {
      const pad = align - rem;
      ftParts.push(Buffer.alloc(pad));
      ftOffset += pad;
    }
  };

  if (draco) {
    ftJson.POINTS_LENGTH = draco.pointsLength;
    const dracoOffset = pushFt(new Uint8Array(draco.buffer));
    ftJson.extensions = {
      '3DTILES_draco_point_compression': {
        properties: draco.featureProperties,
        byteOffset: dracoOffset,
        byteLength: draco.buffer.length,
      },
    };
  } else if (quantized) {
    ftJson.POINTS_LENGTH = quantized.values.length / 3;
    ftJson.QUANTIZED_VOLUME_OFFSET = quantized.offset;
    ftJson.QUANTIZED_VOLUME_SCALE = quantized.scale;
    ftJson.POSITION_QUANTIZED = { byteOffset: pushFt(new Uint16Array(quantized.values)) };
  } else {
    ftJson.POINTS_LENGTH = positions.length / 3;
    ftJson.POSITION = { byteOffset: pushFt(new Float32Array(positions)) };
  }
  if (rtcCenter) ftJson.RTC_CENTER = rtcCenter;
  if (rgb) ftJson.RGB = { byteOffset: pushFt(new Uint8Array(rgb)) };
  if (rgba) ftJson.RGBA = { byteOffset: pushFt(new Uint8Array(rgba)) };
  if (rgb565) {
    alignFt(2);
    ftJson.RGB565 = { byteOffset: pushFt(new Uint16Array(rgb565)) };
  }
  if (constantRgba) ftJson.CONSTANT_RGBA = constantRgba;
  if (normals) {
    alignFt(4);
    ftJson.NORMAL = { byteOffset: pushFt(new Float32Array(normals)) };
  }
  if (normalsOct16p) {
    ftJson.NORMAL_OCT16P = { byteOffset: pushFt(new Uint8Array(normalsOct16p)) };
  }
  if (batchIds) {
    ftJson.BATCH_ID = { byteOffset: pushFt(new Uint8Array(batchIds)) };
    ftJson.BATCH_LENGTH = batchLength;
  }

  let btJsonObj = batchTableJson ? { ...batchTableJson } : null;
  if (draco && draco.batchProperties) {
    btJsonObj = btJsonObj ?? {};
    btJsonObj.extensions = {
      '3DTILES_draco_point_compression': {
        properties: draco.batchProperties,
      },
    };
  }

  const pad8 = (buf, fill) => {
    const len = Math.ceil(buf.length / 8) * 8;
    const out = Buffer.alloc(len, fill);
    buf.copy(out);
    return out;
  };
  const ftJsonBuf = pad8(Buffer.from(JSON.stringify(ftJson)), 0x20);
  const ftBinBuf = pad8(Buffer.concat(ftParts), 0);
  const btJsonBuf = btJsonObj ? pad8(Buffer.from(JSON.stringify(btJsonObj)), 0x20) : Buffer.alloc(0);
  const btBinBuf = batchTableBinary ? pad8(Buffer.from(batchTableBinary), 0) : Buffer.alloc(0);
  const byteLength = 28 + ftJsonBuf.length + ftBinBuf.length + btJsonBuf.length + btBinBuf.length;
  const header = Buffer.alloc(28);
  header.write('pnts', 0, 'ascii');
  header.writeUInt32LE(1, 4);
  header.writeUInt32LE(byteLength, 8);
  header.writeUInt32LE(ftJsonBuf.length, 12);
  header.writeUInt32LE(ftBinBuf.length, 16);
  header.writeUInt32LE(btJsonBuf.length, 20);
  header.writeUInt32LE(btBinBuf.length, 24);
  return Buffer.concat([header, ftJsonBuf, ftBinBuf, btJsonBuf, btBinBuf]);
}

// Helper: build a binary .subtree file (3D Tiles 1.1)
function buildSubtreeBinary({ tileBits, contentBits, childSubtreeConstant = 0 }) {
  const tileBytes = Buffer.from(tileBits);
  const contentBytes = Buffer.from(contentBits);
  const binRaw = Buffer.concat([tileBytes, contentBytes]);
  const binLen = Math.ceil(binRaw.length / 8) * 8;
  const binBuf = Buffer.alloc(binLen, 0);
  binRaw.copy(binBuf);

  const jsonObj = {
    buffers: [{ byteLength: binRaw.length }],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: tileBytes.length },
      { buffer: 0, byteOffset: tileBytes.length, byteLength: contentBytes.length },
    ],
    tileAvailability: { bitstream: 0, availableCount: 2 },
    contentAvailability: [{ bitstream: 1, availableCount: 2 }],
    childSubtreeAvailability: { constant: childSubtreeConstant },
  };
  const jsonRaw = Buffer.from(JSON.stringify(jsonObj));
  const jsonLen = Math.ceil(jsonRaw.length / 8) * 8;
  const jsonBuf = Buffer.alloc(jsonLen, 0x20);
  jsonRaw.copy(jsonBuf);

  const header = Buffer.alloc(24);
  header.write('subt', 0, 'ascii');
  header.writeUInt32LE(1, 4);
  header.writeBigUInt64LE(BigInt(jsonLen), 8);
  header.writeBigUInt64LE(BigInt(binLen), 16);
  return Buffer.concat([header, jsonBuf, binBuf]);
}

// Helper: build a binary .glb voxel tile with EXT_structural_metadata propertyAttributes
function buildVoxelGlb({ temperatures, rgb }) {
  const count = temperatures.length;
  const tempArr = new Float32Array(temperatures);
  const rgbArr = new Uint8Array(rgb);
  const tempBytes = Buffer.from(tempArr.buffer);
  const rgbBytes = Buffer.from(rgbArr.buffer);
  const binRaw = Buffer.concat([tempBytes, rgbBytes]);
  const binLen = Math.ceil(binRaw.length / 4) * 4;
  const binBuf = Buffer.alloc(binLen, 0);
  binRaw.copy(binBuf);

  const gltf = {
    asset: { version: '2.0' },
    extensionsUsed: ['EXT_structural_metadata'],
    extensions: {
      EXT_structural_metadata: {
        schema: {
          id: 'voxel-schema',
          classes: {
            voxel: {
              properties: {
                temperature: { type: 'SCALAR', componentType: 'FLOAT32' },
                rgb: { type: 'VEC3', componentType: 'UINT8' },
              },
            },
          },
        },
        propertyAttributes: [
          {
            class: 'voxel',
            properties: {
              temperature: { attribute: '_TEMPERATURE' },
              rgb: { attribute: '_RGB' },
            },
          },
        ],
      },
    },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0 }],
    meshes: [
      {
        primitives: [
          {
            attributes: { _TEMPERATURE: 0, _RGB: 1 },
            extensions: { EXT_structural_metadata: { propertyAttributes: [0] } },
          },
        ],
      },
    ],
    buffers: [{ byteLength: binRaw.length }],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: tempBytes.length },
      { buffer: 0, byteOffset: tempBytes.length, byteLength: rgbBytes.length },
    ],
    accessors: [
      { bufferView: 0, componentType: 5126, count, type: 'SCALAR' },
      { bufferView: 1, componentType: 5121, count, type: 'VEC3' },
    ],
  };
  const jsonRaw = Buffer.from(JSON.stringify(gltf));
  const jsonLen = Math.ceil(jsonRaw.length / 4) * 4;
  const jsonBuf = Buffer.alloc(jsonLen, 0x20);
  jsonRaw.copy(jsonBuf);

  const totalLen = 12 + 8 + jsonLen + 8 + binLen;
  const header = Buffer.alloc(12);
  header.write('glTF', 0, 'ascii');
  header.writeUInt32LE(2, 4);
  header.writeUInt32LE(totalLen, 8);

  const jsonChunkHdr = Buffer.alloc(8);
  jsonChunkHdr.writeUInt32LE(jsonLen, 0);
  jsonChunkHdr.writeUInt32LE(0x4e4f534a, 4);

  const binChunkHdr = Buffer.alloc(8);
  binChunkHdr.writeUInt32LE(binLen, 0);
  binChunkHdr.writeUInt32LE(0x004e4942, 4);

  return Buffer.concat([header, jsonChunkHdr, jsonBuf, binChunkHdr, binBuf]);
}

// Build voxel tileset assets
const voxelBoxTileset = {
  asset: { version: '1.1' },
  schema: {
    id: 'voxel-schema',
    classes: {
      voxel: {
        properties: {
          temperature: { type: 'SCALAR', componentType: 'FLOAT32' },
          rgb: { type: 'VEC3', componentType: 'UINT8' },
        },
      },
      tilesetMeta: {
        properties: {
          tileCount: { type: 'SCALAR', componentType: 'UINT32', semantic: 'TILESET_TILE_COUNT' },
        },
      },
    },
  },
  metadata: {
    class: 'tilesetMeta',
    properties: { tileCount: 9 },
  },
  statistics: {
    classes: {
      voxel: {
        properties: {
          temperature: { min: -10.5, max: 42.5 },
          rgb: { min: [0, 10, 20], max: [255, 240, 230] },
        },
      },
    },
  },
  geometricError: 100.0,
  root: {
    boundingVolume: {
      box: [10, 20, 30, 0, 4, 0, -2, 0, 0, 0, 0, 6],
    },
    transform: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 100, 200, 300, 1],
    geometricError: 10.0,
    refine: 'REPLACE',
    content: {
      uri: 'content/{level}_{x}_{y}_{z}.glb',
      extensions: {
        '3DTILES_content_voxels': {
          dimensions: [2, 2, 2],
          padding: { before: [1, 0, 0], after: [0, 1, 0] },
          class: 'voxel',
        },
      },
    },
    implicitTiling: {
      subdivisionScheme: 'OCTREE',
      subtreeLevels: 2,
      availableLevels: 2,
      subtrees: { uri: 'subtrees/{level}.{x}.{y}.{z}.subtree' },
    },
  },
};

const voxelRegionTileset = {
  asset: { version: '1.1' },
  schema: voxelBoxTileset.schema,
  geometricError: 100.0,
  root: {
    boundingVolume: { region: [-0.5, -0.25, 0.5, 0.25, 0.0, 500.0] },
    geometricError: 10.0,
    content: {
      uri: 'content/{level}_{x}_{y}_{z}.glb',
      extensions: {
        '3DTILES_content_voxels': { dimensions: [2, 2, 2], class: 'voxel' },
      },
    },
    implicitTiling: {
      subdivisionScheme: 'OCTREE',
      subtreeLevels: 1,
      availableLevels: 1,
      subtrees: { uri: 'subtrees/{level}.{x}.{y}.{z}.subtree' },
    },
  },
};

const voxelCylinderTileset = {
  asset: { version: '1.1' },
  schema: voxelBoxTileset.schema,
  geometricError: 100.0,
  root: {
    boundingVolume: {
      extensions: {
        '3DTILES_bounding_volume_cylinder': {
          minRadius: 1.0,
          maxRadius: 12.0,
          height: 20.0,
          minAngle: -1.5,
          maxAngle: 1.5,
          translation: [2.0, -3.0, 4.0],
          rotation: [0.0, 0.0, 0.7071067811865475, 0.7071067811865476],
        },
      },
    },
    geometricError: 10.0,
    content: {
      uri: 'content/{level}_{x}_{y}_{z}.glb',
      extensions: {
        '3DTILES_content_voxels': { dimensions: [2, 2, 2], class: 'voxel' },
      },
    },
    implicitTiling: {
      subdivisionScheme: 'OCTREE',
      subtreeLevels: 1,
      availableLevels: 1,
      subtrees: { uri: 'subtrees/{level}.{x}.{y}.{z}.subtree' },
    },
  },
};

// Octree subtree with 2 levels has 1 + 8 = 9 tiles -> 2 bytes of bitstream.
// Bit 0 = root (0,0,0,0) -> 1; Bit 1 = child (1,0,0,0) -> 1; others -> 0.
const subtree0Bytes = buildSubtreeBinary({
  tileBits: [0b00000011, 0b00000000],
  contentBits: [0b00000011, 0b00000000],
  childSubtreeConstant: 0,
});
const glb0Bytes = buildVoxelGlb({
  temperatures: [-5.5, 0.0, 12.25, 20.5, 31.0, -1.25, 42.5, 10.0],
  rgb: [
    10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120,
    130, 140, 150, 160, 170, 180, 190, 200, 210, 220, 230, 240,
  ],
});
const glb1Bytes = buildVoxelGlb({
  temperatures: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0],
  rgb: [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
  ],
});

// Build PointCloud test buffers
const pntsBasic = buildPnts({
  positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
  rgb: [255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128],
  rtcCenter: [10.0, 20.0, 30.0],
});
const pntsQuantized = buildPnts({
  quantized: {
    values: [0, 0, 0, 65535, 32768, 16384, 1000, 2000, 3000, 65535, 65535, 65535],
    offset: [-5.0, -10.0, -15.0],
    scale: [10.0, 20.0, 30.0],
  },
  rgb565: [0xf800, 0x07e0, 0x001f, 0xffff],
  normalsOct16p: [128, 128, 255, 128, 0, 128, 128, 255],
});
const pntsRgba = buildPnts({
  positions: [0, 0, 0, 2, 0, 0, 0, 2, 0],
  rgba: [255, 0, 0, 128, 0, 255, 0, 200, 0, 0, 255, 255],
});
const pntsBatchIds = buildPnts({
  positions: [0, 0, 0, 1, 2, 3, -1, -2, -3, 4, 5, 6],
  rgb: [200, 100, 50, 50, 100, 200, 10, 20, 30, 250, 240, 230],
  normals: [0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0, -1],
  batchIds: [0, 1, 0, 1],
  batchLength: 2,
  batchTableJson: { name: ['zero', 'one'] },
});

// Binary batch-table styleable properties (no BATCH_ID)
const tempBuf = Buffer.from(new Float32Array([10.0, 25.5, 40.0, 5.0]).buffer);
const velBuf = Buffer.from(new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 0.5, 0.5, 0.5]).buffer);
const flagBuf = Buffer.from(new Uint8Array([1, 2, 3, 4]).buffer);
const btBinary = Buffer.concat([tempBuf, velBuf, flagBuf]);
const pntsBinaryBatchStyleable = buildPnts({
  positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1],
  rgb: [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
  batchTableJson: {
    temperature: { byteOffset: 0, componentType: 'FLOAT', type: 'SCALAR' },
    velocity: { byteOffset: tempBuf.length, componentType: 'FLOAT', type: 'VEC3' },
    unusedFlag: { byteOffset: tempBuf.length + velBuf.length, componentType: 'UNSIGNED_BYTE', type: 'SCALAR' },
  },
  batchTableBinary: btBinary,
});

// Draco point clouds from pinned decodeDraco.json cases 56 and 60
const dracoCase56Bytes = decodeDracoFixture.cases[56].a[0].buffer;
const pntsDracoQuantizedOct = buildPnts({
  draco: {
    pointsLength: 40,
    buffer: dracoCase56Bytes,
    featureProperties: { POSITION: 0, NORMAL: 1, RGB: 2 },
  },
});
const dracoCase60Bytes = decodeDracoFixture.cases[60].a[0].buffer;
const pntsDracoBatchStyleable = buildPnts({
  draco: {
    pointsLength: 3,
    buffer: dracoCase60Bytes,
    featureProperties: { POSITION: 0 },
    batchProperties: { extra: 2 },
  },
});

// TimeDynamicPointCloud frames (different sizes/positions)
const tdFrame0 = buildPnts({
  positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
  rgb: [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
});
const tdFrame1 = buildPnts({
  positions: [5, 0, 0, 6, 0, 0, 5, 1, 0, 5, 0, 1, 7, 1, 1, 8, 2, 2],
  rgb: [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180],
});
const tdFrame2 = buildPnts({
  positions: [-5, 0, 0, -4, 0, 0, -5, 2, 0],
  rgb: [200, 200, 0, 0, 200, 200, 200, 0, 200],
});

const staticFiles = new Map([
  ['/voxels/tileset.json', { ct: 'application/json', body: Buffer.from(JSON.stringify(voxelBoxTileset)) }],
  ['/voxels/tileset-region.json', { ct: 'application/json', body: Buffer.from(JSON.stringify(voxelRegionTileset)) }],
  ['/voxels/tileset-cylinder.json', { ct: 'application/json', body: Buffer.from(JSON.stringify(voxelCylinderTileset)) }],
  ['/voxels/subtrees/0.0.0.0.subtree', { ct: 'application/octet-stream', body: subtree0Bytes }],
  ['/voxels/content/0_0_0_0.glb', { ct: 'application/octet-stream', body: glb0Bytes }],
  ['/voxels/content/1_0_0_0.glb', { ct: 'application/octet-stream', body: glb1Bytes }],
  ['/td/frame-0.pnts', { ct: 'application/octet-stream', body: tdFrame0 }],
  ['/td/frame-1.pnts', { ct: 'application/octet-stream', body: tdFrame1 }],
  ['/td/frame-2.pnts', { ct: 'application/octet-stream', body: tdFrame2 }],
  ['/draco_decoder.wasm', { ct: 'application/wasm', body: dracoWasmBytes }],
]);

const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') {
    res.setHeader('Content-Type', 'text/javascript');
    res.end(bundle.outputFiles[0].contents);
    return;
  }
  const hit = staticFiles.get(req.url);
  if (hit) {
    res.setHeader('Content-Type', hit.ct);
    res.end(hit.body);
    return;
  }
  if (req.url === '/') {
    res.end('<!doctype html><title>Cluster 5 PointCloud & Voxel GPU validation</title>');
    return;
  }
  res.statusCode = 404;
  res.end('Not found');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const baseUrl = `http://127.0.0.1:${server.address().port}`;

const pntsPayloadB64 = {
  basic: pntsBasic.toString('base64'),
  quantized: pntsQuantized.toString('base64'),
  rgba: pntsRgba.toString('base64'),
  batchIds: pntsBatchIds.toString('base64'),
  binaryBatchStyleable: pntsBinaryBatchStyleable.toString('base64'),
  dracoQuantizedOct: pntsDracoQuantizedOct.toString('base64'),
  dracoBatchStyleable: pntsDracoBatchStyleable.toString('base64'),
  tdFrame0: tdFrame0.toString('base64'),
  tdFrame1: tdFrame1.toString('base64'),
  tdFrame2: tdFrame2.toString('base64'),
};

let browser;
try {
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH,
    headless: true,
    args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  const page = await browser.newPage();
  await page.goto(`${baseUrl}/`);

  const result = await page.evaluate(
    async ({ baseUrl, pntsPayloadB64 }) => {
      try {
        const ref = await import('/reference.mjs');

        // Initialize real CesiumJS decodeDraco WASM module directly in-page so
        // PointCloud.update runs 100% real Cesium Draco decoding without needing a worker bundle.
        const wasmResp = await fetch(`${baseUrl}/draco_decoder.wasm`);
        const wasmBinary = await wasmResp.arrayBuffer();
        await ref.decodeDraco({ webAssemblyConfig: { wasmBinary, wasmBinaryFile: `${baseUrl}/draco_decoder.wasm` } });
        ref.DracoLoader.decodePointCloud = function (parameters) {
          return Promise.resolve(ref.decodeDraco(parameters));
        };

        const fnv = (bytes) => {
          let h = 0xcbf29ce484222325n;
          for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
          return h.toString(16);
        };
        const fromBase64 = (b64) => {
          const bin = atob(b64);
          const bytes = new Uint8Array(bin.length);
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
          return bytes;
        };
        const makeCanvas = (w, h) => {
          const c = document.createElement('canvas');
          c.width = w;
          c.height = h;
          return c;
        };
        const attrs = { alpha: true, depth: true, stencil: true, antialias: false, preserveDrawingBuffer: true };
        const packMat4 = (m) => (m ? Array.from(ref.Matrix4.pack(m, [])) : null);
        const packMat3 = (m) => (m ? Array.from(ref.Matrix3.pack(m, [])) : null);
        const packC3 = (v) => (v ? [v.x, v.y, v.z] : null);
        const packC4 = (v) => (v ? [v.x, v.y, v.z, v.w] : null);
        const packC2 = (v) => (v ? [v.x, v.y] : null);
        const packSphere = (s) => (s ? { center: packC3(s.center), radius: s.radius } : null);

        // =====================================================================
        // 1. Megatexture on SwiftShader WebGL2
        // =====================================================================
        const megatexture = (() => {
          const canvas = makeCanvas(8, 8);
          const context = new ref.Context(canvas, { webgl: attrs });
          const gl = context._gl;
          const uploads = [];
          const origSub3D = gl.texSubImage3D.bind(gl);
          gl.texSubImage3D = function (target, level, xoff, yoff, zoff, w, h, d, fmt, type, src) {
            const bytes = new Uint8Array(src.buffer, src.byteOffset, src.byteLength);
            uploads.push({
              xOffset: xoff,
              yOffset: yoff,
              zOffset: zoff,
              width: w,
              height: h,
              depth: d,
              format: fmt,
              type,
              byteLength: bytes.byteLength,
              hash: fnv(bytes),
            });
            return origSub3D(target, level, xoff, yoff, zoff, w, h, d, fmt, type, src);
          };

          const m1 = new ref.Megatexture(
            context,
            new ref.Cartesian3(4, 4, 4),
            1,
            ref.MetadataComponentType.FLOAT32,
            1024 * 1024,
            4,
          );
          const m1SamplerLinear = {
            min: m1.texture.sampler.minificationFilter,
            mag: m1.texture.sampler.magnificationFilter,
          };
          m1.nearestSampling = true;
          const m1SamplerNearest = {
            min: m1.texture.sampler.minificationFilter,
            mag: m1.texture.sampler.magnificationFilter,
          };
          m1.nearestSampling = false;
          const m1Info = {
            channelCount: m1.channelCount,
            textureMemoryByteLength: m1.textureMemoryByteLength,
            tileCounts: packC3(m1.tileCounts),
            voxelCountPerTile: packC3(m1.voxelCountPerTile),
            maximumTileCount: m1.maximumTileCount,
            textureDims: [m1.texture.width, m1.texture.height, m1.texture.depth],
            pixelFormat: m1.texture.pixelFormat,
            pixelDatatype: m1.texture.pixelDatatype,
            samplerLinear: m1SamplerLinear,
            samplerNearest: m1SamplerNearest,
          };
          m1.destroy();

          uploads.length = 0;
          const m2 = new ref.Megatexture(
            context,
            new ref.Cartesian3(2, 3, 2),
            4,
            ref.MetadataComponentType.UINT8,
            1024 * 1024,
            6,
          );
          const makeTile = (seed) => {
            const a = new Uint8Array(2 * 3 * 2 * 4);
            for (let i = 0; i < a.length; i++) a[i] = (seed * 31 + i) & 0xff;
            return a;
          };
          const seq = [];
          seq.push({ op: 'add', slot: m2.add(makeTile(1)), occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(2)), occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(3)), occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(4)), occupied: m2.occupiedCount, full: m2.isFull() });
          m2.remove(1);
          seq.push({ op: 'remove', slot: 1, occupied: m2.occupiedCount, full: m2.isFull() });
          m2.remove(3);
          seq.push({ op: 'remove', slot: 3, occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(5)), occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(6)), occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(7)), occupied: m2.occupiedCount, full: m2.isFull() });
          seq.push({ op: 'add', slot: m2.add(makeTile(8)), occupied: m2.occupiedCount, full: m2.isFull() });

          let fullError = null;
          try {
            m2.add(makeTile(9));
          } catch (e) {
            fullError = e.message;
          }
          let oobError = null;
          try {
            m2.remove(99);
          } catch (e) {
            oobError = e.message;
          }

          const m2Info = {
            tileCounts: packC3(m2.tileCounts),
            maximumTileCount: m2.maximumTileCount,
            textureDims: [m2.texture.width, m2.texture.height, m2.texture.depth],
            textureMemoryByteLength: m2.textureMemoryByteLength,
            pixelFormat: m2.texture.pixelFormat,
            pixelDatatype: m2.texture.pixelDatatype,
            seq,
            uploads: uploads.slice(),
            fullError,
            oobError,
          };
          m2.destroy();
          const m2Destroyed = m2.isDestroyed();

          const m3 = new ref.Megatexture(
            context,
            new ref.Cartesian3(2, 2, 2),
            2,
            ref.MetadataComponentType.FLOAT64,
            1024 * 1024,
            2,
          );
          const m3Info = {
            textureDims: [m3.texture.width, m3.texture.height, m3.texture.depth],
            pixelFormat: m3.texture.pixelFormat,
            pixelDatatype: m3.texture.pixelDatatype,
            maximumTileCount: m3.maximumTileCount,
          };
          m3.destroy();

          const glError = gl.getError();
          const max3D = ref.ContextLimits.maximum3DTextureSize;
          context.destroy();
          return { max3D, m1Info, m2Info, m2Destroyed, m3Info, glError };
        })();

        // =====================================================================
        // 2. Cesium3DTilesVoxelProvider end-to-end (tileset + subtree + GLB)
        // =====================================================================
        const cesium3DTilesVoxelProvider = await (async () => {
          const canvas = makeCanvas(8, 8);
          const context = new ref.Context(canvas, { webgl: attrs });

          const snapProvider = (p) => ({
            className: p.className,
            names: [...p.names],
            types: [...p.types],
            componentTypes: [...p.componentTypes],
            shape: p.shape,
            dimensions: packC3(p.dimensions),
            paddingBefore: packC3(p.paddingBefore),
            paddingAfter: packC3(p.paddingAfter),
            globalTransform: packMat4(p.globalTransform),
            shapeTransform: packMat4(p.shapeTransform),
            minBounds: packC3(p.minBounds),
            maxBounds: packC3(p.maxBounds),
            minimumValues: p.minimumValues ?? null,
            maximumValues: p.maximumValues ?? null,
            maximumTileCount: p.maximumTileCount ?? null,
            availableLevels: p.availableLevels ?? null,
            metadataOrder: p.metadataOrder,
          });

          const boxProvider = await ref.Cesium3DTilesVoxelProvider.fromUrl(`${baseUrl}/voxels/tileset.json`);
          const regionProvider = await ref.Cesium3DTilesVoxelProvider.fromUrl(`${baseUrl}/voxels/tileset-region.json`);
          const cylinderProvider = await ref.Cesium3DTilesVoxelProvider.fromUrl(
            `${baseUrl}/voxels/tileset-cylinder.json`,
          );

          const loadTile = async (provider, coords) => {
            const content = await provider.requestData(coords);
            const dummyFrame = {
              context,
              creditDisplay: { addCreditToNextFrame() {} },
              passes: { render: true },
              afterRender: [],
            };
            const dummyPrim = { provider };
            for (let i = 0; i < 30 && !content.ready; i++) {
              content.update(dummyPrim, dummyFrame);
              await new Promise((r) => setTimeout(r, 5));
            }
            const meta = content.metadata.map((arr) => ({
              type: arr.constructor.name,
              values: Array.from(arr),
            }));
            content.destroy();
            return meta;
          };

          const tile0 = await loadTile(boxProvider, { tileLevel: 0, tileX: 0, tileY: 0, tileZ: 0 });
          const tile1 = await loadTile(boxProvider, { tileLevel: 1, tileX: 0, tileY: 0, tileZ: 0 });

          let unavailableError = null;
          try {
            await boxProvider.requestData({ tileLevel: 1, tileX: 1, tileY: 1, tileZ: 1 });
          } catch (e) {
            unavailableError = typeof e === 'string' ? e : e.message;
          }
          let keyframeError = null;
          try {
            await boxProvider.requestData({ tileLevel: 0, tileX: 0, tileY: 0, tileZ: 0, keyframe: 1 });
          } catch (e) {
            keyframeError = typeof e === 'string' ? e : e.message;
          }

          context.destroy();
          return {
            box: snapProvider(boxProvider),
            region: snapProvider(regionProvider),
            cylinder: snapProvider(cylinderProvider),
            tile0,
            tile1,
            unavailableError,
            keyframeError,
          };
        })();

        // =====================================================================
        // 3. VoxelPrimitive on SwiftShader WebGL2
        // =====================================================================
        const voxelPrimitive = await (async () => {
          const canvas = makeCanvas(64, 64);
          const context = new ref.Context(canvas, { webgl: attrs });
          const gl = context._gl;

          const makeCameraAndFrame = (pos, dir, up, opts = {}) => {
            const frustum = new ref.PerspectiveFrustum({
              fov: Math.PI / 3,
              aspectRatio: 1.0,
              near: 0.1,
              far: 100.0,
            });
            const view = ref.Matrix4.computeView(
              pos,
              dir,
              up,
              ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
              new ref.Matrix4(),
            );
            const camera = {
              positionWC: pos,
              directionWC: dir,
              upWC: up,
              rightWC: ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
              viewMatrix: view,
              inverseViewMatrix: ref.Matrix4.inverse(view, new ref.Matrix4()),
              frustum,
            };
            const cullingVolume = frustum.computeCullingVolume(pos, dir, up);
            const frameState = {
              context,
              camera,
              cullingVolume,
              pixelRatio: 1.0,
              frameNumber: opts.frameNumber ?? 1,
              mode: ref.SceneMode.SCENE3D,
              useLogDepth: opts.useLogDepth ?? false,
              verticalExaggeration: opts.verticalExaggeration ?? 1.0,
              verticalExaggerationRelativeHeight: opts.verticalExaggerationRelativeHeight ?? 0.0,
              passes: {
                render: opts.render ?? true,
                pick: opts.pick ?? false,
                pickVoxel: opts.pickVoxel ?? false,
              },
              commandList: [],
              afterRender: [],
              time: new ref.JulianDate(2451545, 0),
              mapProjection: new ref.GeographicProjection(),
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
            };
            context.uniformState.update(frameState);
            context.uniformState.updateCamera(camera);
            context.uniformState.updateFrustum(frustum);
            return frameState;
          };

          // Case A: Box provider with 2 properties, padding, statistics
          const boxProvider = {
            shape: ref.VoxelShapeType.BOX,
            dimensions: new ref.Cartesian3(2, 2, 2),
            paddingBefore: new ref.Cartesian3(1, 0, 0),
            paddingAfter: new ref.Cartesian3(0, 1, 0),
            names: ['density', 'color'],
            types: [ref.MetadataType.SCALAR, ref.MetadataType.VEC3],
            componentTypes: [ref.MetadataComponentType.FLOAT32, ref.MetadataComponentType.UINT8],
            minimumValues: [[0.0], [0, 0, 0]],
            maximumValues: [[1.0], [255, 255, 255]],
            availableLevels: 2,
            maximumTileCount: 9,
            globalTransform: ref.Matrix4.IDENTITY,
            shapeTransform: ref.Matrix4.IDENTITY,
            metadataOrder: ref.VoxelMetadataOrder.Y_UP,
            requestData(options) {
              if (options.tileLevel >= 2) return undefined;
              const n = 3 * 3 * 2; // (2+1+0)*(2+0+1)*(2+0+0) = 18 voxels
              const density = new Float32Array(n).fill(0.5);
              const color = new Uint8Array(n * 3).fill(200);
              return Promise.resolve(ref.VoxelContent.fromMetadataArray([density, color]));
            },
          };
          const primA = new ref.VoxelPrimitive({ provider: boxProvider, calculateStatistics: true });
          const camPos = new ref.Cartesian3(0, 0, 5);
          const camDir = new ref.Cartesian3(0, 0, -1);
          const camUp = new ref.Cartesian3(0, 1, 0);

          let fs = makeCameraAndFrame(camPos, camDir, camUp, { frameNumber: 1 });
          primA.update(fs);
          for (const cb of fs.afterRender) cb();
          // Pump frames so tile promise resolves and megatexture + draw commands build
          for (let f = 2; f <= 5; f++) {
            fs = makeCameraAndFrame(camPos, camDir, camUp, { frameNumber: f });
            primA.update(fs);
            for (const cb of fs.afterRender) cb();
            await new Promise((r) => setTimeout(r, 5));
          }

          const renderCmd = fs.commandList[0];
          const snapVoxelCmd = (cmd) => ({
            pass: cmd.pass,
            cull: cmd.cull,
            occlude: cmd.occlude,
            pickOnly: cmd.pickOnly,
            executeInClosestFrustum: cmd.executeInClosestFrustum,
            boundingVolume: packSphere(cmd.boundingVolume),
            renderState: {
              cullEnabled: cmd.renderState.cull.enabled,
              cullFace: cmd.renderState.cull.face,
              depthTestEnabled: cmd.renderState.depthTest.enabled,
              depthMask: cmd.renderState.depthMask,
              blendingEnabled: cmd.renderState.blending.enabled,
              blendingSrcRgb: cmd.renderState.blending.functionSourceRgb,
              blendingDstRgb: cmd.renderState.blending.functionDestinationRgb,
            },
            fragmentDefines: [...cmd.shaderProgram.fragmentShaderSource.defines].sort(),
          });

          const renderSnap = snapVoxelCmd(renderCmd);
          const uniformsA = {
            u_dimensions: packC3(primA._uniforms.dimensions),
            u_inputDimensions: packC3(primA._uniforms.inputDimensions),
            u_paddingBefore: packC3(primA._uniforms.paddingBefore),
            u_paddingAfter: packC3(primA._uniforms.paddingAfter),
            u_megatextureTileCounts: packC3(primA._uniforms.megatextureTileCounts),
            u_octreeInternalNodeTilesPerRow: primA._uniforms.octreeInternalNodeTilesPerRow,
            u_octreeInternalNodeTexelSizeUv: packC2(primA._uniforms.octreeInternalNodeTexelSizeUv),
            u_ndcSpaceAxisAlignedBoundingBox: packC4(primA._uniforms.ndcSpaceAxisAlignedBoundingBox),
            u_transformPositionViewToLocal: packMat4(primA._uniforms.transformPositionViewToLocal),
            u_transformDirectionViewToLocal: packMat3(primA._uniforms.transformDirectionViewToLocal),
            u_cameraPositionLocal: packC3(primA._uniforms.cameraPositionLocal),
            u_cameraDirectionLocal: packC3(primA._uniforms.cameraDirectionLocal),
            u_cameraTileCoordinates: packC4(primA._uniforms.cameraTileCoordinates),
            u_cameraTileUv: packC3(primA._uniforms.cameraTileUv),
            u_stepSize: primA._uniforms.stepSize,
          };

          // Pick pass
          const fsPick = makeCameraAndFrame(camPos, camDir, camUp, { frameNumber: 6, render: false, pick: true });
          primA.update(fsPick);
          const pickSnap = snapVoxelCmd(fsPick.commandList[0]);

          // PickVoxel pass
          const fsPickVoxel = makeCameraAndFrame(camPos, camDir, camUp, {
            frameNumber: 7,
            render: false,
            pick: false,
            pickVoxel: true,
          });
          primA.update(fsPickVoxel);
          const pickVoxelSnap = snapVoxelCmd(fsPickVoxel.commandList[0]);

          // Offscreen culling: camera looking away (+Z)
          const fsOff = makeCameraAndFrame(new ref.Cartesian3(0, 0, 5), new ref.Cartesian3(0, 0, 1), camUp, {
            frameNumber: 8,
          });
          primA.update(fsOff);
          const offscreenCommands = fsOff.commandList.length;

          // show = false
          primA.show = false;
          const fsHidden = makeCameraAndFrame(camPos, camDir, camUp, { frameNumber: 9 });
          primA.update(fsHidden);
          const hiddenCommands = fsHidden.commandList.length;
          primA.show = true;

          // depthTest = false -> shader rebuilds without DEPTH_TEST
          primA.depthTest = false;
          primA.nearestSampling = true;
          const fsNoDepth = makeCameraAndFrame(camPos, camDir, camUp, { frameNumber: 10 });
          primA.update(fsNoDepth);
          const noDepthDefines = [...fsNoDepth.commandList[0].shaderProgram.fragmentShaderSource.defines].sort();
          const nearestAfter = primA._traversal.megatextures.every((m) => m.nearestSampling);

          const caseA = {
            ready: primA.ready,
            dimensions: packC3(primA.dimensions),
            inputDimensions: packC3(primA.inputDimensions),
            paddingBefore: packC3(primA.paddingBefore),
            paddingAfter: packC3(primA.paddingAfter),
            boundingSphere: packSphere(primA.boundingSphere),
            texturesByteLength: primA.statistics.texturesByteLength,
            renderSnap,
            pickSnap,
            pickVoxelSnap,
            uniforms: uniformsA,
            offscreenCommands,
            hiddenCommands,
            noDepthDefines,
            nearestAfter,
          };
          primA.destroy();

          // Case B: Ellipsoid + ClippingPlanes + CustomShader uniform + logDepth + verticalExaggeration
          const ellipProvider = {
            shape: ref.VoxelShapeType.ELLIPSOID,
            dimensions: new ref.Cartesian3(2, 2, 2),
            names: ['val'],
            types: [ref.MetadataType.SCALAR],
            componentTypes: [ref.MetadataComponentType.FLOAT32],
            availableLevels: 1,
            maximumTileCount: 1,
            globalTransform: ref.Matrix4.IDENTITY,
            shapeTransform: ref.Matrix4.fromScale(new ref.Cartesian3(6378137, 6378137, 6356752.314245179)),
            minBounds: new ref.Cartesian3(-Math.PI, -Math.PI / 2, 0),
            maxBounds: new ref.Cartesian3(Math.PI, Math.PI / 2, 100000),
            metadataOrder: ref.VoxelMetadataOrder.Z_UP,
            requestData(options) {
              if (options.tileLevel >= 1) return undefined;
              return Promise.resolve(ref.VoxelContent.fromMetadataArray([new Float32Array(8).fill(0.25)]));
            },
          };
          const customShader = new ref.CustomShader({
            uniforms: { u_scale: { type: ref.UniformType.FLOAT, value: 2.5 } },
            fragmentShaderText:
              'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { material.diffuse = vec3(u_scale); material.alpha = 1.0; }',
          });
          const primB = new ref.VoxelPrimitive({ provider: ellipProvider, customShader });
          primB.clippingPlanes = new ref.ClippingPlaneCollection({
            planes: [new ref.ClippingPlane(new ref.Cartesian3(0, 0, 1), 0.0)],
            enabled: true,
          });
          const ePos = new ref.Cartesian3(0, 0, 20000000);
          const eDir = new ref.Cartesian3(0, 0, -1);
          const eUp = new ref.Cartesian3(0, 1, 0);
          const eFrustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 1, near: 1000, far: 1e8 });
          const makeEllipFrame = (frameNumber) => {
            const view = ref.Matrix4.computeView(
              ePos,
              eDir,
              eUp,
              ref.Cartesian3.cross(eDir, eUp, new ref.Cartesian3()),
              new ref.Matrix4(),
            );
            const camera = {
              positionWC: ePos,
              directionWC: eDir,
              upWC: eUp,
              rightWC: ref.Cartesian3.cross(eDir, eUp, new ref.Cartesian3()),
              viewMatrix: view,
              inverseViewMatrix: ref.Matrix4.inverse(view, new ref.Matrix4()),
              frustum: eFrustum,
            };
            const f = {
              context,
              camera,
              cullingVolume: eFrustum.computeCullingVolume(ePos, eDir, eUp),
              pixelRatio: 1.0,
              frameNumber,
              mode: ref.SceneMode.SCENE3D,
              useLogDepth: true,
              verticalExaggeration: 2.0,
              verticalExaggerationRelativeHeight: 0.0,
              passes: { render: true, pick: false, pickVoxel: false },
              commandList: [],
              afterRender: [],
              time: new ref.JulianDate(2451545, 0),
              mapProjection: new ref.GeographicProjection(),
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
            };
            context.uniformState.update(f);
            context.uniformState.updateCamera(camera);
            context.uniformState.updateFrustum(eFrustum);
            return f;
          };
          let fb = makeEllipFrame(1);
          primB.update(fb);
          for (const cb of fb.afterRender) cb();
          for (let f = 2; f <= 5; f++) {
            fb = makeEllipFrame(f);
            primB.update(fb);
            for (const cb of fb.afterRender) cb();
            await new Promise((r) => setTimeout(r, 5));
          }
          const caseB = {
            inputDimensions: packC3(primB.inputDimensions),
            fragmentDefines: [...fb.commandList[0].shaderProgram.fragmentShaderSource.defines].sort(),
            clippingPlanesMatrix: packMat4(primB._uniforms.clippingPlanesMatrix),
            u_scale: fb.commandList[0].uniformMap.u_scale(),
          };
          primB.destroy();

          const glError = gl.getError();
          context.destroy();
          return { caseA, caseB, glError };
        })();

        // =====================================================================
        // 4. PointCloud on SwiftShader WebGL2 (all variants incl. binary batch-table & Draco)
        // =====================================================================
        const pointCloud = await (async () => {
          const canvas = makeCanvas(64, 64);
          const context = new ref.Context(canvas, { webgl: attrs });
          const gl = context._gl;

          const frustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 1, near: 0.1, far: 1000 });
          const pos = new ref.Cartesian3(0, 0, 10);
          const dir = new ref.Cartesian3(0, 0, -1);
          const up = new ref.Cartesian3(0, 1, 0);
          const view = ref.Matrix4.computeView(
            pos,
            dir,
            up,
            ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
            new ref.Matrix4(),
          );
          const camera = {
            positionWC: pos,
            directionWC: dir,
            upWC: up,
            rightWC: ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
            viewMatrix: view,
            inverseViewMatrix: ref.Matrix4.inverse(view, new ref.Matrix4()),
            frustum,
          };
          const makeFrame = () => {
            const f = {
              context,
              camera,
              mode: ref.SceneMode.SCENE3D,
              pixelRatio: 2.0,
              drawingBufferHeight: 64,
              commandList: [],
              passes: { render: true, pick: false },
              mapProjection: new ref.GeographicProjection(),
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
              afterRender: [],
              time: new ref.JulianDate(2451545, 0),
            };
            context.uniformState.update(f);
            context.uniformState.updateCamera(camera);
            context.uniformState.updateFrustum(frustum);
            return f;
          };

          const specs = [
            { name: 'basic-rtc', b64: pntsPayloadB64.basic, modelTranslation: [100, 200, 300] },
            {
              name: 'quantized-rgb565-oct16p',
              b64: pntsPayloadB64.quantized,
              attenuation: true,
              backFaceCulling: true,
              geometricError: 2.5,
              geometricErrorScale: 1.5,
              maximumAttenuation: 8.0,
            },
            { name: 'rgba-translucent', b64: pntsPayloadB64.rgba },
            { name: 'batch-ids', b64: pntsPayloadB64.batchIds, opaquePass: ref.Pass.CESIUM_3D_TILE },
            {
              name: 'binary-batch-table-styleable',
              b64: pntsPayloadB64.binaryBatchStyleable,
              style: {
                color: 'vec4(${temperature} / 50.0, ${velocity}.x, ${COLOR}.b, 1.0)',
                show: '${temperature} > 8.0',
                pointSize: '4.0 + ${velocity}.y * 2.0',
              },
            },
            {
              name: 'draco-quantized-oct-normal',
              b64: pntsPayloadB64.dracoQuantizedOct,
              normalShading: true,
              backFaceCulling: true,
            },
            {
              name: 'draco-batch-table-styleable',
              b64: pntsPayloadB64.dracoBatchStyleable,
              style: {
                color: '${extra} > 0.5 ? color("red") : color("blue")',
              },
            },
          ];

          const cases = [];
          for (const spec of specs) {
            const bytes = fromBase64(spec.b64);
            let batchTableCb = null;
            const cloud = new ref.PointCloud({
              arrayBuffer: bytes.buffer,
              byteOffset: 0,
              cull: true,
              opaquePass: spec.opaquePass ?? ref.Pass.OPAQUE,
              pickIdLoaded: () => 'czm_pickColor',
              batchTableLoaded: (len, json, bin) => {
                batchTableCb = { len, hasJson: !!json, hasBin: !!bin };
              },
            });
            if (spec.modelTranslation) {
              cloud.modelMatrix = ref.Matrix4.fromTranslation(new ref.Cartesian3(...spec.modelTranslation));
            }
            if (spec.attenuation !== undefined) cloud.attenuation = spec.attenuation;
            if (spec.backFaceCulling !== undefined) cloud.backFaceCulling = spec.backFaceCulling;
            if (spec.normalShading !== undefined) cloud.normalShading = spec.normalShading;
            if (spec.geometricError !== undefined) cloud.geometricError = spec.geometricError;
            if (spec.geometricErrorScale !== undefined) cloud.geometricErrorScale = spec.geometricErrorScale;
            if (spec.maximumAttenuation !== undefined) cloud.maximumAttenuation = spec.maximumAttenuation;
            if (spec.style) cloud.style = new ref.Cesium3DTileStyle(spec.style);

            let frame = makeFrame();
            cloud.update(frame);
            if (!cloud.ready) {
              await new Promise((r) => setTimeout(r, 20));
              frame = makeFrame();
              cloud.update(frame);
            }
            const cmd = cloud._drawCommand;
            // Execute draw command on SwiftShader WebGL2!
            const passState = new ref.PassState(context);
            passState.viewport = new ref.BoundingRectangle(0, 0, 64, 64);
            new ref.ClearCommand({ color: ref.Color.BLACK, depth: 1.0 }).execute(context, passState);
            cmd.execute(context, passState);

            const attrsSnap = [];
            for (let i = 0; i < cmd.vertexArray.numberOfAttributes; i++) {
              const a = cmd.vertexArray.getAttribute(i);
              attrsSnap.push({
                index: a.index,
                enabled: a.enabled,
                componentsPerAttribute: a.componentsPerAttribute,
                componentDatatype: a.componentDatatype,
                normalize: a.normalize,
              });
            }
            const um = cmd.uniformMap;
            const uPt = um.u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier();
            const uQ = um.u_quantizedVolumeScaleAndOctEncodedRange
              ? um.u_quantizedVolumeScaleAndOctEncodedRange()
              : null;

            cases.push({
              name: spec.name,
              b64: spec.b64,
              pointsLength: cloud.pointsLength,
              geometryByteLength: cloud.geometryByteLength,
              ready: cloud.ready,
              isTranslucent: cloud._isTranslucent,
              styleTranslucent: cloud._styleTranslucent,
              isQuantizedDraco: cloud._isQuantizedDraco,
              isOctEncodedDraco: cloud._isOctEncodedDraco,
              quantizedRange: cloud._quantizedRange,
              octEncodedRange: cloud._octEncodedRange,
              modelMatrix: packMat4(cmd.modelMatrix),
              boundingSphere: packSphere(cmd.boundingVolume),
              pass: cmd.pass,
              attributes: attrsSnap,
              vertexShader: cmd.shaderProgram.vertexShaderSource.sources.join('\n'),
              fragmentShader: cmd.shaderProgram.fragmentShaderSource.sources.join('\n'),
              uniformNames: Object.keys(um).sort(),
              u_pointSizeAndTimeAndGeometricErrorAndDepthMultiplier: packC4(uPt),
              u_quantizedVolumeScaleAndOctEncodedRange: packC4(uQ),
              batchTableCb,
              glError: gl.getError(),
            });
            cloud.destroy();
          }
          context.destroy();
          return cases;
        })();

        // =====================================================================
        // 5. PointCloudEyeDomeLighting on SwiftShader WebGL2
        // =====================================================================
        const pointCloudEyeDomeLighting = (() => {
          const canvas = makeCanvas(32, 24);
          const context = new ref.Context(canvas, { webgl: attrs });
          const gl = context._gl;

          const supported = ref.PointCloudEyeDomeLighting.isSupported(context);
          const edl = new ref.PointCloudEyeDomeLighting();

          const frustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 32 / 24, near: 0.1, far: 100 });
          const pos = new ref.Cartesian3(0, 0, 5);
          const dir = new ref.Cartesian3(0, 0, -1);
          const up = new ref.Cartesian3(0, 1, 0);
          const view = ref.Matrix4.computeView(
            pos,
            dir,
            up,
            ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
            new ref.Matrix4(),
          );
          const camera = {
            positionWC: pos,
            directionWC: dir,
            upWC: up,
            rightWC: ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
            viewMatrix: view,
            inverseViewMatrix: ref.Matrix4.inverse(view, new ref.Matrix4()),
            frustum,
          };
          const frame = {
            context,
            camera,
            mode: ref.SceneMode.SCENE3D,
            pixelRatio: 2.0,
            drawingBufferHeight: 24,
            commandList: [],
            passes: { render: true, pick: false },
            mapProjection: new ref.GeographicProjection(),
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
            afterRender: [],
            time: new ref.JulianDate(2451545, 0),
          };
          context.uniformState.update(frame);
          context.uniformState.updateCamera(camera);
          context.uniformState.updateFrustum(frustum);

          // Build 3 commands:
          // [0]: pre-existing opaque point cloud (before commandStart=1 -> must NOT be hijacked)
          // [1]: opaque point cloud (after commandStart=1 -> MUST be hijacked)
          // [2]: translucent point cloud (Pass.TRANSLUCENT -> must NOT be hijacked)
          // [3]: triangle draw command (PrimitiveType.TRIANGLES -> must NOT be hijacked)
          const mkCloud = (b64) => {
            const c = new ref.PointCloud({
              arrayBuffer: fromBase64(b64).buffer,
              cull: true,
              opaquePass: ref.Pass.CESIUM_3D_TILE,
              pickIdLoaded: () => 'czm_pickColor',
            });
            const f = { ...frame, commandList: [] };
            c.update(f);
            return c;
          };
          const cPre = mkCloud(pntsPayloadB64.basic);
          const cOpaque = mkCloud(pntsPayloadB64.basic);
          const cTrans = mkCloud(pntsPayloadB64.rgba);
          const triCmd = context.createViewportQuadCommand('void main() { out_FragColor = vec4(1.0); }', {
            pass: ref.Pass.CESIUM_3D_TILE,
          });
          frame.commandList.push(cPre._drawCommand, cOpaque._drawCommand, cTrans._drawCommand, triCmd);

          const shading = new ref.PointCloudShading({
            attenuation: true,
            eyeDomeLighting: true,
            eyeDomeLightingStrength: 1.75,
            eyeDomeLightingRadius: 2.5,
          });
          const bv = new ref.BoundingSphere(new ref.Cartesian3(1, 2, 3), 15.0);
          edl.update(frame, 1, shading, bv);

          // Execute the derived point draw, blend command, and clear command on SwiftShader WebGL2!
          const passState = new ref.PassState(context);
          passState.viewport = new ref.BoundingRectangle(0, 0, 32, 24);
          const derivedOpaque = frame.commandList[1];
          const blendCmd = frame.commandList[4];
          const clearCmd = frame.commandList[5];
          // Clear offscreen FBO first, render points into it, blend to default FBO, clear offscreen FBO
          clearCmd.execute(context, passState);
          derivedOpaque.execute(context, passState);
          blendCmd.execute(context, passState);
          clearCmd.execute(context, passState);

          const out = {
            supported,
            framebufferDims: [edl.colorGBuffer.width, edl.colorGBuffer.height],
            colorAttachmentCount: edl.framebuffer.numberOfColorAttachments,
            commandListLength: frame.commandList.length,
            preUntouched: frame.commandList[0] === cPre._drawCommand,
            opaqueReplaced: frame.commandList[1] !== cOpaque._drawCommand,
            opaqueTargetsEdlFb: derivedOpaque.framebuffer === edl.framebuffer,
            opaqueCastShadows: derivedOpaque.castShadows,
            opaqueReceiveShadows: derivedOpaque.receiveShadows,
            derivedFragmentSources: [...derivedOpaque.shaderProgram.fragmentShaderSource.sources],
            translucentUntouched: frame.commandList[2] === cTrans._drawCommand,
            triangleUntouched: frame.commandList[3] === triCmd,
            blendPass: blendCmd.pass,
            blendBoundingVolume: packSphere(blendCmd.boundingVolume),
            blendDefines: [...blendCmd.shaderProgram.fragmentShaderSource.defines],
            blendDistanceAndStrength: packC2(blendCmd.uniformMap.u_distanceAndEdlStrength()),
            blendRenderState: {
              blendingEnabled: blendCmd.renderState.blending.enabled,
              depthMask: blendCmd.renderState.depthMask,
              depthTestEnabled: blendCmd.renderState.depthTest.enabled,
              stencilTestEnabled: blendCmd.renderState.stencilTest.enabled,
              stencilMask: blendCmd.renderState.stencilMask,
            },
            clearTargetsEdlFb: clearCmd.framebuffer === edl.framebuffer,
            clearPass: clearCmd.pass,
            clearColor: [clearCmd.color.red, clearCmd.color.green, clearCmd.color.blue, clearCmd.color.alpha],
            clearDepth: clearCmd.depth,
            glError: gl.getError(),
          };
          cPre.destroy();
          cOpaque.destroy();
          cTrans.destroy();
          edl.destroy();
          out.destroyed = edl.isDestroyed();
          context.destroy();
          return out;
        })();

        // =====================================================================
        // 6. TimeDynamicPointCloud end-to-end on SwiftShader WebGL2
        // =====================================================================
        const timeDynamicPointCloud = await (async () => {
          const canvas = makeCanvas(32, 32);
          const context = new ref.Context(canvas, { webgl: attrs });
          const gl = context._gl;

          const start = new ref.JulianDate(2451545, 0);
          const t10 = ref.JulianDate.addSeconds(start, 10, new ref.JulianDate());
          const t20 = ref.JulianDate.addSeconds(start, 20, new ref.JulianDate());
          const t30 = ref.JulianDate.addSeconds(start, 30, new ref.JulianDate());

          const frame1Transform = ref.Matrix4.pack(ref.Matrix4.fromTranslation(new ref.Cartesian3(50, 0, 0)), []);
          const intervals = new ref.TimeIntervalCollection([
            new ref.TimeInterval({
              start,
              stop: t10,
              isStartIncluded: true,
              isStopIncluded: false,
              data: { uri: `${baseUrl}/td/frame-0.pnts` },
            }),
            new ref.TimeInterval({
              start: t10,
              stop: t20,
              isStartIncluded: true,
              isStopIncluded: false,
              data: { uri: `${baseUrl}/td/frame-1.pnts`, transform: frame1Transform },
            }),
            new ref.TimeInterval({
              start: t20,
              stop: t30,
              isStartIncluded: true,
              isStopIncluded: true,
              data: { uri: `${baseUrl}/td/frame-2.pnts` },
            }),
          ]);

          const clock = new ref.Clock({
            startTime: start,
            stopTime: t30,
            currentTime: ref.JulianDate.addSeconds(start, 5, new ref.JulianDate()),
            clockRange: ref.ClockRange.CLAMPED,
            multiplier: 10.0,
            shouldAnimate: true,
            canAnimate: true,
          });

          const td = new ref.TimeDynamicPointCloud({
            clock,
            intervals,
            modelMatrix: ref.Matrix4.fromTranslation(new ref.Cartesian3(10, 0, 0)),
            shading: { attenuation: true, eyeDomeLighting: true, eyeDomeLightingStrength: 1.5, eyeDomeLightingRadius: 2.0 },
          });
          let frameChangedCount = 0;
          td.frameChanged.addEventListener(() => {
            frameChangedCount++;
          });

          const frustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 1, near: 0.1, far: 1000 });
          const pos = new ref.Cartesian3(0, 0, 50);
          const dir = new ref.Cartesian3(0, 0, -1);
          const up = new ref.Cartesian3(0, 1, 0);
          const view = ref.Matrix4.computeView(
            pos,
            dir,
            up,
            ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
            new ref.Matrix4(),
          );
          const camera = {
            positionWC: pos,
            directionWC: dir,
            upWC: up,
            rightWC: ref.Cartesian3.cross(dir, up, new ref.Cartesian3()),
            viewMatrix: view,
            inverseViewMatrix: ref.Matrix4.inverse(view, new ref.Matrix4()),
            frustum,
          };
          let fn = 1;
          const step = (time, extra = {}) => {
            const f = {
              context,
              camera,
              mode: extra.mode ?? ref.SceneMode.SCENE3D,
              pixelRatio: 1.0,
              drawingBufferHeight: 32,
              frameNumber: fn++,
              time,
              commandList: [],
              passes: { render: true, pick: false },
              mapProjection: new ref.GeographicProjection(),
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
              afterRender: [],
              time: new ref.JulianDate(2451545, 0),
            };
            context.uniformState.update(f);
            context.uniformState.updateCamera(camera);
            context.uniformState.updateFrustum(frustum);
            td.update(f);
            for (const cb of f.afterRender) cb();
            return f;
          };

          // Initial update starts loading frame 0 & prefetching frame 1
          let f = step(clock.currentTime);
          await new Promise((r) => setTimeout(r, 40));
          f = step(clock.currentTime);
          const afterFrame0 = {
            commandListLength: f.commandList.length, // 1 derived point + 1 EDL blend + 1 EDL clear = 3
            totalMemoryUsageInBytes: td.totalMemoryUsageInBytes,
            boundingSphere: packSphere(td.boundingSphere),
            frameChangedCount,
            edlUsed: f.commandList.length === 3 && f.commandList[2].color !== undefined,
          };

          // Advance clock into interval 1 (prefetched!)
          clock.currentTime = ref.JulianDate.addSeconds(start, 15, new ref.JulianDate());
          f = step(clock.currentTime);
          await new Promise((r) => setTimeout(r, 40));
          f = step(clock.currentTime);
          const afterFrame1 = {
            commandListLength: f.commandList.length,
            totalMemoryUsageInBytes: td.totalMemoryUsageInBytes,
            boundingSphere: packSphere(td.boundingSphere),
            frameChangedCount,
          };

          // Force memory eviction by shrinking maximumMemoryUsage and moving to interval 2
          td.maximumMemoryUsage = 0.00001; // ~10 bytes
          clock.currentTime = ref.JulianDate.addSeconds(start, 25, new ref.JulianDate());
          f = step(clock.currentTime);
          await new Promise((r) => setTimeout(r, 40));
          f = step(clock.currentTime);
          const afterEviction = {
            commandListLength: f.commandList.length,
            totalMemoryUsageInBytes: td.totalMemoryUsageInBytes,
            boundingSphere: packSphere(td.boundingSphere),
            frameChangedCount,
          };

          // Hidden & morphing emit 0 commands
          td.show = false;
          const hiddenCmds = step(clock.currentTime).commandList.length;
          td.show = true;
          const morphingCmds = step(clock.currentTime, { mode: ref.SceneMode.MORPHING }).commandList.length;

          // Missing URI triggers frameFailed
          const failIntervals = new ref.TimeIntervalCollection([
            new ref.TimeInterval({
              start,
              stop: t10,
              isStartIncluded: true,
              isStopIncluded: true,
              data: { uri: `${baseUrl}/td/missing.pnts` },
            }),
          ]);
          const failClock = new ref.Clock({
            startTime: start,
            stopTime: t10,
            currentTime: ref.JulianDate.addSeconds(start, 2, new ref.JulianDate()),
            shouldAnimate: false,
          });
          const tdFail = new ref.TimeDynamicPointCloud({ clock: failClock, intervals: failIntervals });
          const failures = [];
          tdFail.frameFailed.addEventListener((e) => failures.push(e.uri));
          const failFrame = {
            context,
            camera,
            mode: ref.SceneMode.SCENE3D,
            pixelRatio: 1.0,
            drawingBufferHeight: 32,
            frameNumber: fn++,
            time: failClock.currentTime,
            commandList: [],
            passes: { render: true, pick: false },
            mapProjection: new ref.GeographicProjection(),
              fog: { enabled: false, density: 0, sse: 0, minimumBrightness: 0 },
              backgroundColor: ref.Color.BLACK,
              light: { color: ref.Color.WHITE, intensity: 2.0, direction: ref.Cartesian3.UNIT_Z },
              atmosphere: { hueShift: 0, saturationShift: 0, brightnessShift: 0, rayleighCoefficient: new ref.Cartesian3(5.5e-6, 13.0e-6, 28.4e-6), mieCoefficient: new ref.Cartesian3(21e-6, 21e-6, 21e-6), rayleighScaleHeight: 10000, mieScaleHeight: 3200, mieAnisotropy: 0.9, dynamicLighting: 0 },
              shadowState: { shadowsEnabled: false, lightShadowsEnabled: false, shadowMaps: [], lightShadowMaps: [] },
              invertClassification: false,
              invertClassificationColor: ref.Color.WHITE,
            afterRender: [],
          };
          tdFail.update(failFrame);
          await new Promise((r) => setTimeout(r, 40));
          tdFail.destroy();

          const glError = gl.getError();
          td.destroy();
          const destroyed = td.isDestroyed();
          context.destroy();
          return {
            afterFrame0,
            afterFrame1,
            afterEviction,
            hiddenCmds,
            morphingCmds,
            failuresCount: failures.length,
            destroyed,
            glError,
          };
        })();

        return {
          version: '23.0.0',
          megatexture,
          cesium3DTilesVoxelProvider,
          voxelPrimitive,
          pointCloud,
          pointCloudEyeDomeLighting,
          timeDynamicPointCloud,
        };
      } catch (e) {
        return { __error: e?.stack ?? String(e) };
      }
    },
    { baseUrl, pntsPayloadB64 },
  );

  if (result.__error) {
    throw new Error(result.__error);
  }

  // Embed binary fixtures needed by Rust replay
  result.assets = {
    voxelBoxTileset,
    voxelRegionTileset,
    voxelCylinderTileset,
    subtree0Base64: subtree0Bytes.toString('base64'),
    glb0Base64: glb0Bytes.toString('base64'),
    glb1Base64: glb1Bytes.toString('base64'),
    tdFrame0Base64: tdFrame0.toString('base64'),
    tdFrame1Base64: tdFrame1.toString('base64'),
    tdFrame2Base64: tdFrame2.toString('base64'),
  };

  writeFileSync(
    new URL('../../tests/data/scene-pointcloud-voxel-gpu.json', import.meta.url),
    JSON.stringify(result, null, 2) + '\n',
  );
  console.log('Wrote tests/data/scene-pointcloud-voxel-gpu.json successfully.');
} finally {
  if (browser) await browser.close();
  server.close();
}
