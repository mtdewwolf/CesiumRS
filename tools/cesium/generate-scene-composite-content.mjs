import { writeFileSync } from 'node:fs';
import Composite from '@cesium/engine/Source/Scene/Composite3DTileContent.js';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Ray from '@cesium/engine/Source/Core/Ray.js';

const calls = [];
const children = [4, 2].map((x, i) => ({
  ready: i === 0, featurePropertiesDirty: false,
  update() { calls.push(`update${i}`); },
  pick(_ray, _frame, result) { return Cartesian3.clone(new Cartesian3(x, 0, 0), result); },
  destroy() { calls.push(`destroy${i}`); },
}));
const c = new Composite({}, {}, new Resource({url:'https://example.com/tile.cmpt?x=1'}), children);
const ray = new Ray(Cartesian3.ZERO, Cartesian3.UNIT_X);
const result = { initialReady:c.ready, ownCounts:[c.featuresLength,c.pointsLength,c.trianglesLength,c.geometryByteLength,c.texturesByteLength,c.batchTableByteLength], url:c.url };
c.update({}, {}); result.firstReady = c.ready;
children[1].ready = true; c.update({}, {}); result.secondReady = c.ready;
result.pick = c.pick(ray, {}, new Cartesian3()).x;
children[0].ready = false; c.update({}, {}); result.stickyReady = c.ready;
c.destroy(); result.calls = calls; result.destroyed = c.isDestroyed();
const buffer = new ArrayBuffer(40);
const bytes = new Uint8Array(buffer);
const view = new DataView(buffer);
bytes.set(new TextEncoder().encode('cmpt'), 0);
view.setUint32(4, 1, true); view.setUint32(8, 40, true); view.setUint32(12, 2, true);
for (const [offset, magic] of [[16, 'b3dm'], [28, 'vctr']]) {
  bytes.set(new TextEncoder().encode(magic), offset); view.setUint32(offset + 8, 12, true);
}
const offsets = [];
const factory = Object.fromEntries(['b3dm', 'vctr'].map(magic => [magic, (_tileset, _tile, resource, _buffer, offset) => {
  offsets.push({magic, offset, url: resource.getUrlComponent(true)}); return {};
}]));
await Composite.fromTileType({}, {}, new Resource({url:'https://example.com/tile.cmpt?compositeIndex=3&x=1'}), buffer, 0, factory);
result.children = offsets;
view.setUint32(4, 2, true);
try { await Composite.fromTileType({}, {}, new Resource({url:'https://example.com/tile.cmpt'}), buffer, 0, factory); }
catch (error) { result.versionError = error.message; }
writeFileSync(new URL('../../tests/data/scene-composite-content.json', import.meta.url), JSON.stringify(result, null, 2)+'\n');
