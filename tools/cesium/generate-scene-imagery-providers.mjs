// Generates pinned-source fixtures for the imagery-provider batch.
//
//   node tools/cesium/generate-scene-imagery-providers.mjs [Module ...]
//
// This is intentionally separate from fixtures/generate.mjs: the shared
// generator is owned by the parent integration change. Every expected value
// below comes from executing @cesium/engine 23.0.0 with deterministic Resource
// transports/patches; no network is used.

import {mkdirSync, writeFileSync, readFileSync} from 'node:fs';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {DOMParser} from '@xmldom/xmldom';

import Bing from './node_modules/@cesium/engine/Source/Scene/BingMapsImageryProvider.js';
import BingStyle from './node_modules/@cesium/engine/Source/Scene/BingMapsStyle.js';
import GeeMetadata from './node_modules/@cesium/engine/Source/Core/GoogleEarthEnterpriseMetadata.js';
import ArcGis from './node_modules/@cesium/engine/Source/Scene/ArcGisMapServerImageryProvider.js';
import Wms from './node_modules/@cesium/engine/Source/Scene/WebMapServiceImageryProvider.js';
import Wmts from './node_modules/@cesium/engine/Source/Scene/WebMapTileServiceImageryProvider.js';
import Tms from './node_modules/@cesium/engine/Source/Scene/TileMapServiceImageryProvider.js';
import Google2D from './node_modules/@cesium/engine/Source/Scene/Google2DImageryProvider.js';
import Azure from './node_modules/@cesium/engine/Source/Scene/Azure2DImageryProvider.js';
import GeeMaps from './node_modules/@cesium/engine/Source/Scene/GoogleEarthEnterpriseMapsProvider.js';
import GeeImagery from './node_modules/@cesium/engine/Source/Scene/GoogleEarthEnterpriseImageryProvider.js';
import Grid from './node_modules/@cesium/engine/Source/Scene/GridImageryProvider.js';
import TileCoordinates from './node_modules/@cesium/engine/Source/Scene/TileCoordinatesImageryProvider.js';
import ImageryProvider from './node_modules/@cesium/engine/Source/Scene/ImageryProvider.js';
import Never from './node_modules/@cesium/engine/Source/Scene/NeverTileDiscardPolicy.js';
import Resource from './node_modules/@cesium/engine/Source/Core/Resource.js';
import Credit from './node_modules/@cesium/engine/Source/Core/Credit.js';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '../../tests/fixtures/scene');
const version = JSON.parse(readFileSync(join(here, './node_modules/@cesium/engine/package.json'), 'utf8')).version;
const selected = new Set(process.argv.slice(2));
if (typeof globalThis.document === 'undefined') globalThis.document = {location: {protocol: 'https:'}};
if (typeof globalThis.Image === 'undefined') globalThis.Image = class Image {};
Bing.logoUrl = 'Assets/Images/bing_maps_credit.png';
GeeMaps.logoUrl = 'Assets/Images/google_earth_credit.png';
const files = {};

function suite(module, cases) {
  files[module] = {module, cesium: version, cases};
}
function rect(r) {
  return [r.west, r.south, r.east, r.north];
}
function credit(c) {
  return c ? {html: c.html, showOnScreen: c.showOnScreen} : null;
}
async function withPatches(patches, fn) {
  const restore = [];
  for (const [owner, key, value] of patches) {
    const had = Object.prototype.hasOwnProperty.call(owner, key);
    const old = owner[key];
    owner[key] = value;
    restore.push(() => {
      if (had) owner[key] = old;
      else delete owner[key];
    });
  }
  try {
    return await fn();
  } finally {
    for (const r of restore) r();
  }
}

function metadataOf(p) {
  return {
    rectangle: rect(p.rectangle),
    tilingSchemeRectangle: rect(p.tilingScheme.rectangle),
    tileWidth: p.tileWidth,
    tileHeight: p.tileHeight,
    minimumLevel: p.minimumLevel ?? null,
    maximumLevel: p.maximumLevel ?? null,
    hasAlphaChannel: p.hasAlphaChannel,
    credit: credit(p.credit),
    discardPolicy: p.tileDiscardPolicy ? p.tileDiscardPolicy.constructor.name : null,
  };
}

// ------------------------------------------------------------------ Bing
{
  const metadata = {
    resourceSets: [
      {
        resources: [
          {
            imageWidth: 256,
            imageHeight: 300,
            zoomMax: 20,
            imageUrlSubdomains: ['t0', 't1', 't2'],
            imageUrl: 'https://{subdomain}.tiles.example.com/tiles/{quadkey}?c={culture}',
            imageryProviders: [
              {
                attribution: 'Provider A',
                coverageAreas: [
                  {bbox: [-10, 20, 10, 40], zoomMin: 1, zoomMax: 10},
                  {bbox: [20, 10, 40, 30], zoomMin: 2, zoomMax: 20},
                ],
              },
              {
                attribution: 'Ignored',
                coverageAreas: [{zoomMin: 1, zoomMax: 20}],
              },
            ],
          },
        ],
      },
    ],
  };
  async function provider(name) {
    const options =
      name === 'custom'
        ? {
            key: 'bing-key',
            mapStyle: BingStyle.AERIAL_WITH_LABELS,
            mapLayer: 'labels',
            culture: 'en-US',
            tileDiscardPolicy: new Never(),
            tileProtocol: 'http:',
          }
        : {key: 'bing-key', tileDiscardPolicy: new Never()};
    return withPatches(
      [[Resource.prototype, 'fetchJson', async () => metadata]],
      () => Bing.fromUrl(Resource.createIfNeeded('https://dev.virtualearth.net/base'), options),
    );
  }
  const cases = [];
  for (const name of ['defaults', 'custom']) {
    const p = await provider(name);
    cases.push({f: 'metadata', a: [name], r: {...metadataOf(p), url: p.url, key: p.key, mapStyle: p.mapStyle, mapLayer: p.mapLayer ?? null, culture: p.culture}});
  }
  cases.push({
    f: 'quadKeys',
    a: [],
    r: [
      [0, 0, 0, Bing.tileXYToQuadKey(0, 0, 0)],
      [3, 5, 4, Bing.tileXYToQuadKey(3, 5, 4)],
      [5, 3, 4, Bing.tileXYToQuadKey(5, 3, 4)],
      [1, 2, 8, Bing.tileXYToQuadKey(1, 2, 8)],
      ['12021', Bing.quadKeyToTileXY('12021')],
    ],
  });
  const p = await provider('defaults');
  const tiles = [[0, 0, 1], [1, 2, 3], [5, 7, 4], [12, 9, 5]];
  let captured = '';
  await withPatches([[ImageryProvider, 'loadImage', (_p, resource) => {
    captured = resource.url;
    return Promise.resolve('image');
  }]], async () => {
    for (const [x, y, level] of tiles) {
      p.requestImage(x, y, level);
      cases.push({f: 'tileUrls', a: [x, y, level], r: captured});
    }
  });
  cases.push({
    f: 'credits',
    a: [],
    r: tiles.map(([x, y, level]) => (p.getTileCredits(x, y, level) ?? []).map(credit)),
  });
  suite('BingMapsImageryProvider', cases);
}

// ------------------------------------------------------------------ ArcGIS
{
  const tiledMetadata = {
    tileInfo: {
      rows: 256,
      cols: 256,
      spatialReference: {wkid: 102100},
      lods: [{level: 0}, {level: 1}, {level: 2}, {level: 3}],
    },
    fullExtent: {xmin: -20037508, ymin: -20037508, xmax: 20037508, ymax: 20037508, spatialReference: {wkid: 102100}},
    copyrightText: 'ArcGIS Copyright',
  };
  const exportMetadata = {copyrightText: 'Export copyright'};
  async function provider(name) {
    const useTiles = name !== 'export';
    return withPatches(
      [[Resource.prototype, 'fetchJson', async () => (useTiles ? tiledMetadata : exportMetadata)]],
      () =>
        ArcGis.fromUrl(Resource.createIfNeeded('https://services.example.com/arcgis/rest/services/Test/MapServer'), {
          token: 'arc-token',
          usePreCachedTilesIfAvailable: useTiles,
          tileDiscardPolicy: new Never(),
          layers: name === 'export' ? '0,1' : undefined,
          tileWidth: 512,
          tileHeight: 256,
          maximumLevel: name === 'export' ? 7 : undefined,
        }),
    );
  }
  const cases = [];
  for (const name of ['tiled', 'export']) {
    const p = await provider(name);
    cases.push({
      f: 'metadata',
      a: [name],
      r: {
        ...metadataOf(p),
        url: p.url,
        token: p.token ?? null,
        usingPrecachedTiles: p.usingPrecachedTiles,
        layers: p.layers ?? null,
        tileCredits: (p.getTileCredits(0, 0, 0) ?? []).map(credit),
      },
    });
    let captured = '';
    await withPatches([[ImageryProvider, 'loadImage', (_p, resource) => {
      captured = resource.url;
      return Promise.resolve('image');
    }]], async () => {
      p.requestImage(1, 2, 3);
      cases.push({f: 'tileUrls', a: [name, 1, 2, 3], r: captured});
    });
  }
  const p = await provider('tiled');
  const identifyJson = {
    results: [
      {
        value: 'Alpha',
        attributes: {name: 'Alpha', count: 2},
        geometryType: 'esriGeometryPoint',
        geometry: {x: 10.5, y: -20.25, spatialReference: {wkid: 4326}},
      },
      {
        value: 'Mercator',
        attributes: {title: 'Mercator'},
        geometryType: 'esriGeometryPoint',
        geometry: {x: 0, y: 0, spatialReference: {wkid: 102100}},
      },
      {attributes: {name: 'No geometry'}},
    ],
  };
  let identifyUrl = '';
  const features = await withPatches(
    [[Resource.prototype, 'fetchJson', function () {
      identifyUrl = this.url;
      return Promise.resolve(identifyJson);
    }]],
    () => p.pickFeatures(1, 2, 3, 0.5, -0.25),
  );
  cases.push({f: 'pickUrl', a: [], r: identifyUrl});
  cases.push({
    f: 'pickFeatures',
    a: [],
    r: {
      results: identifyJson.results,
      features: features.map((f) => ({
        name: f.name,
        description: f.description,
        position: f.position ? [f.position.longitude, f.position.latitude, f.position.height] : null,
        properties: f.properties,
      })),
    },
  });
  suite('ArcGisMapServerImageryProvider', cases);
}

// ------------------------------------------------------------------ WMS
{
  // Use the real WebMercatorTilingScheme for version13 without importing at top.
  const {default: WebMercatorTilingScheme} = await import('./node_modules/@cesium/engine/Source/Core/WebMercatorTilingScheme.js');
  const cases = [];
  for (const name of ['defaults', 'version13', 'reverseAxis']) {
    let options;
    if (name === 'version13') {
      options = {
        url: Resource.createIfNeeded('https://wms.example.com/service?token=abc'),
        layers: 'layer-a,layer-b',
        tileDiscardPolicy: new Never(),
        parameters: {VERSION: '1.3.0', Format: 'image/png', Transparent: 'TRUE'},
        tilingScheme: new WebMercatorTilingScheme(),
      };
    } else if (name === 'reverseAxis') {
      options = {
        url: Resource.createIfNeeded('https://wms.example.com/service?token=abc'),
        layers: 'layer-a,layer-b',
        tileDiscardPolicy: new Never(),
        parameters: {version: '1.3.0'},
        crs: 'EPSG:3043',
      };
    } else {
      options = {
        url: Resource.createIfNeeded('https://wms.example.com/service?token=abc'),
        layers: 'layer-a,layer-b',
        tileDiscardPolicy: new Never(),
      };
    }
    const p = new Wms(options);
    cases.push({f: 'metadata', a: [name], r: {...metadataOf(p), url: p.url, layers: p.layers, getFeatureInfoUrl: p.getFeatureInfoUrl.url}});
    let captured = '';
    await withPatches([[ImageryProvider, 'loadImage', (_p, resource) => {
      captured = resource.url;
      return Promise.resolve('image');
    }]], async () => {
      p.requestImage(1, 2, 3);
      cases.push({f: 'tileUrls', a: [name, 1, 2, 3], r: captured});
    });
  }
  const p = new Wms({
    url: Resource.createIfNeeded('https://wms.example.com/service?token=abc'),
    layers: 'layer-a,layer-b',
    tileDiscardPolicy: new Never(),
  });
  let pickUrl = '';
  const features = await withPatches([[Resource.prototype, 'fetchJson', function () {
    pickUrl = this.url;
    return Promise.resolve({
      type: 'FeatureCollection',
      features: [{type: 'Feature', properties: {name: 'WMS feature'}, geometry: {type: 'Point', coordinates: [0.5, -0.25]}}],
    });
  }]], () => p.pickFeatures(1, 2, 3, 0.5, -0.25));
  cases.push({f: 'pickUrl', a: [], r: pickUrl});
  cases.push({f: 'pickFeatures', a: [], r: features.map((f) => ({name: f.name, description: f.description, position: f.position ? [f.position.longitude, f.position.latitude, f.position.height] : null}))});
  suite('WebMapServiceImageryProvider', cases);
}

// ------------------------------------------------------------------ WMTS
{
  const cases = [];
  const configurations = {
    kvp: {
      url: Resource.createIfNeeded('https://wmts.example.com/service'),
      layer: 'layer',
      style: 'default',
      tileMatrixSetID: 'matrix',
      format: 'image/png',
      maximumLevel: 6,
      tileDiscardPolicy: new Never(),
    },
    rest: {
      url: Resource.createIfNeeded('https://wmts.example.com/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png'),
      layer: 'layer',
      style: 'dark',
      tileMatrixSetID: 'matrix',
      tileMatrixLabels: ['m0', 'm1', 'm2'],
      dimensions: {Time: '2020-01-01'},
      maximumLevel: 6,
      minimumLevel: 1,
      tileDiscardPolicy: new Never(),
    },
  };
  for (const [name, options] of Object.entries(configurations)) {
    const p = new Wmts(options);
    cases.push({
      f: 'metadata',
      a: [name],
      r: {
        ...metadataOf(p),
        url: p.url,
        layer: p.layer,
        style: p.style,
        tileMatrixSetID: p.tileMatrixSetID,
        format: p.format,
      },
    });
    let captured = '';
    await withPatches([[ImageryProvider, 'loadImage', (_p, resource) => {
      captured = resource.url;
      return Promise.resolve('image');
    }]], async () => {
      for (const tile of [[1, 2, 3], [4, 5, 6]]) {
        p.requestImage(...tile);
        cases.push({f: 'tileUrls', a: [name, ...tile], r: captured});
      }
    });
  }
  suite('WebMapTileServiceImageryProvider', cases);
}

// ------------------------------------------------------------------ TMS
{
  const xml = `<TileMap version="1.0.0" tilemapservice="http://example.com">
  <Title>Test</Title>
  <Abstract></Abstract>
  <SRS>EPSG:4326</SRS>
  <BoundingBox minx="-120" miny="20" maxx="-60" maxy="40"/>
  <Origin x="-180" y="90"/>
  <TileFormat width="256" height="300" mime-type="image/jpeg" extension="jpg"/>
  <TileSets profile="global-geodetic">
    <TileSet href="0" units-per-pixel="0.7" order="0"/>
    <TileSet href="1" units-per-pixel="0.35" order="1"/>
    <TileSet href="2" units-per-pixel="0.17" order="2"/>
  </TileSets>
</TileMap>`;
  const cases = [];
  async function provider(name) {
    const patch = name === 'fallback'
      ? [[Resource.prototype, 'fetchXML', async function () {
          const {default: RequestErrorEvent} = await import('./node_modules/@cesium/engine/Source/Core/RequestErrorEvent.js');
          throw new RequestErrorEvent(404, undefined, undefined, this.url);
        }]]
      : [[Resource.prototype, 'fetchXML', async () => new DOMParser().parseFromString(xml, 'text/xml')]];
    return withPatches(patch, () =>
      Tms.fromUrl(Resource.createIfNeeded('https://tms.example.com/tiles'), {
        fileExtension: name === 'fallback' ? 'png' : undefined,
        maximumLevel: name === 'fallback' ? 5 : undefined,
        rectangle: name === 'fallback' ? undefined : undefined,
        tileDiscardPolicy: new Never(),
      }),
    );
  }
  for (const name of ['xml', 'fallback']) {
    const p = await provider(name);
    cases.push({f: 'metadata', a: [name], r: {...metadataOf(p), url: p.url}});
    let captured = '';
    await withPatches([[ImageryProvider, 'loadImage', (_p, resource) => {
      captured = resource.url;
      return Promise.resolve('image');
    }]], async () => {
      p.requestImage(1, 2, 3);
      cases.push({f: 'tileUrls', a: [name, 1, 2, 3], r: captured});
    });
  }
  suite('TileMapServiceImageryProvider', cases);
}

// ------------------------------------------------------------------ Google 2D
{
  const sessionResponse = JSON.stringify({session: 'session-token', expiry: 1234, tileWidth: 512, tileHeight: 256, imageFormat: 'png'});
  const p = await withPatches(
    [
      [Resource, 'post', async () => sessionResponse],
      [Resource.prototype, 'fetchJson', async function () {
        return {copyright: `Copyright ${this.queryParameters.zoom}`};
      }],
    ],
    () =>
      Google2D.fromUrl({
        key: 'google-key',
        url: Resource.createIfNeeded('https://tile.googleapis.com/'),
        mapType: 'roadmap',
        language: 'fr',
        region: 'FR',
        maximumLevel: 2,
        credit: new Credit('Google Test'),
      }),
  );
  const cases = [
    {f: 'metadata', a: [], r: {...metadataOf(p), url: p.url}},
  ];
  let captured = '';
  await withPatches([
    [ImageryProvider, 'loadImage', (_p, resource) => {
      captured = resource.url;
      return Promise.resolve('image');
    }],
    [Resource.prototype, 'fetchJson', async function () {
      return {copyright: `Copyright ${this.queryParameters.zoom}`};
    }],
  ], async () => {
    await p.requestImage(1, 2, 3);
    cases.push({f: 'tileUrls', a: [1, 2, 3], r: captured});
  });
  cases.push({f: 'credits', a: [], r: [0, 1, 2].map((l) => p._attributionsByLevel.get(l).map(credit))});
  suite('Google2DImageryProvider', cases);
}

// ------------------------------------------------------------------ Azure
{
  const p = new Azure({subscriptionKey: 'azure-key', tilesetId: 'microsoft.base.road', maximumLevel: 1, credit: new Credit('Azure Test')});
  const cases = [{f: 'metadata', a: [], r: {...metadataOf(p), url: p.url}}];
  let captured = '';
  await withPatches([
    [ImageryProvider, 'loadImage', (_p, resource) => {
      captured = resource.url;
      return Promise.resolve('image');
    }],
    [Resource.prototype, 'fetchJson', async function () {
      return {copyrights: ['First', `Level ${this.queryParameters.zoom}`]};
    }],
  ], async () => {
    await p.requestImage(1, 2, 3);
    cases.push({f: 'tileUrls', a: [1, 2, 3], r: captured});
  });
  await withPatches([[Resource.prototype, 'fetchJson', async function () {
    return {copyrights: ['First', `Level ${this.queryParameters.zoom}`]};
  }]], () => p.getViewportCredits());
  cases.push({f: 'credits', a: [], r: [0, 1].map((l) => p._attributionsByLevel.get(l).map(credit))});
  suite('Azure2DImageryProvider', cases);
}

// ------------------------------------------------------------------ GEE Maps
{
  const text = `{
  layers: [
    {id: 1002, version: 7, requestType: "ImageryMaps"},
    {id: 1007, version: 2, requestType: "VectorMapsRaster"}
  ],
  projection: "mercator"
}`;
  const p = await withPatches([[Resource.prototype, 'fetchText', async () => text]], () =>
    GeeMaps.fromUrl(Resource.createIfNeeded('https://earth.example.com'), 1002, {maximumLevel: 9}),
  );
  const cases = [
    {f: 'metadata', a: [], r: {...metadataOf(p), url: p.url.url, path: p.path, channel: p.channel, version: p.version, requestType: p.requestType}},
  ];
  let captured = '';
  await withPatches([[ImageryProvider, 'loadImage', (_p, resource) => {
    captured = resource.url;
    return Promise.resolve('image');
  }]], async () => {
    p.requestImage(1, 2, 3);
    cases.push({f: 'tileUrls', a: [1, 2, 3], r: captured});
  });
  suite('GoogleEarthEnterpriseMapsProvider', cases);
}

// ------------------------------------------------------------------ GEE Imagery
{
  // A deterministic metadata object drives the real pinned provider; tile
  // bytes/decoding are exercised by the Rust lifecycle test.
  const metadata = {
    imageryPresent: true,
    protoImagery: false,
    url: 'https://gee.example.com',
    resource: Resource.createIfNeeded('https://gee.example.com/'),
    providers: {3: new Credit('GEE Provider')},
    getTileInformation(x, y, level) {
      if (x === 0 && y === 0 && level === 0) return {hasImagery: () => true, imageryVersion: 2, imageryProvider: 3};
      if (x === 1 && y === 1 && level === 1) return {hasImagery: () => false, imageryVersion: 1, imageryProvider: 0};
      return undefined;
    },
    isValid: () => true,
    populateSubtree: () => undefined,
    key: new Uint8Array([1, 2, 3, 4]),
  };
  const p = GeeImagery.fromMetadata(metadata, {credit: new Credit('GEE test')});
  const cases = [
    {f: 'metadata', a: [], r: {...metadataOf(p), url: p.url}},
    {f: 'tileUrls', a: [0, 0, 0], r: `flatfile?f1-0${GeeMetadata.tileXYToQuadKey(0, 0, 0)}-i.2`},
    {f: 'tileUrls', a: [1, 1, 1], r: null},
    {f: 'credits', a: [], r: [[credit(p.getTileCredits(0, 0, 0)[0])], null]},
  ];
  suite('GoogleEarthEnterpriseImageryProvider', cases);
}

// ------------------------------------------------------------------ procedural metadata
{
  const fakeDocument = {
    createElement() {
      return {
        width: 0,
        height: 0,
        getContext() {
          return {
            fillStyle: '', strokeStyle: '', lineWidth: 0, font: '', textAlign: '',
            fillRect() {}, strokeRect() {}, fillText() {}, moveTo() {}, lineTo() {}, stroke() {},
          };
        },
      };
    },
  };
  const oldDocument = globalThis.document;
  globalThis.document = fakeDocument;
  try {
    const grid = new Grid({cells: 4, tileWidth: 300, tileHeight: 200, canvasSize: 128});
    suite('GridImageryProvider', [
      {f: 'metadata', a: [], r: {...metadataOf(grid), canvasSize: 128}},
    ]);
    const coords = new TileCoordinates({tileWidth: 300, tileHeight: 200});
    suite('TileCoordinatesImageryProvider', [
      {f: 'metadata', a: [], r: metadataOf(coords)},
    ]);
  } finally {
    if (oldDocument === undefined) delete globalThis.document;
    else globalThis.document = oldDocument;
  }
}

mkdirSync(out, {recursive: true});
let total = 0;
for (const [module, doc] of Object.entries(files)) {
  if (selected.size && !selected.has(module)) continue;
  writeFileSync(join(out, `${module}.json`), JSON.stringify(doc) + '\n');
  total += doc.cases.length;
  console.log(`${module}: ${doc.cases.length} cases`);
}
console.log(`${total} cases`);
