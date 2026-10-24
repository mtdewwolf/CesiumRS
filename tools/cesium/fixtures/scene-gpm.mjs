// Fixtures for the NGA_gpm_local data classes and GltfGpmLoader
// (Scene/Model/Extensions/Gpm). Objects are written through their getters.
import AnchorPointDirect from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/AnchorPointDirect.js';
import AnchorPointIndirect from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/AnchorPointIndirect.js';
import CorrelationGroup from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/CorrelationGroup.js';
import Spdcf from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/Spdcf.js';
import GltfGpmLocal from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/GltfGpmLocal.js';
import GltfGpmLoader from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/GltfGpmLoader.js';
import PpeMetadata from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/PpeMetadata.js';
import PpeSource from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/PpeSource.js';
import PpeTexture from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/PpeTexture.js';
import MeshPrimitiveGpmLocal from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/MeshPrimitiveGpmLocal.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Matrix3 from '@cesium/engine/Source/Core/Matrix3.js';
import {suite} from './lib.mjs';

const direct = (p) => ({position: p.position, adjustmentParams: p.adjustmentParams});
const indirect = (p) => ({position: p.position, adjustmentParams: p.adjustmentParams, covarianceMatrix: p.covarianceMatrix});
const spdcf = (s) => ({A: s.A, alpha: s.alpha, beta: s.beta, T: s.T});
const group = (g) => ({groupFlags: g.groupFlags, rotationThetas: g.rotationThetas, params: g.params.map(spdcf)});
const local = (l) => ({
  storageType: l.storageType,
  anchorPointsDirect: l.anchorPointsDirect?.map(direct),
  anchorPointsIndirect: l.anchorPointsIndirect?.map(indirect),
  intraTileCorrelationGroups: l.intraTileCorrelationGroups?.map(group),
  covarianceDirect: l.covarianceDirect,
});
const metadata = (m) => ({min: m.min, max: m.max, source: m.source});
const texture = (t) => ({traits: metadata(t.traits), noData: t.noData, offset: t.offset, scale: t.scale, index: t.index, texCoord: t.texCoord});
const c3 = (a) => Cartesian3.fromArray(a);

export default function gpm() {
  const ad = suite('AnchorPointDirect');
  for (const [p, q] of [[[1, 2, 3], [0.1, -0.2, 0.3]], [[-6378137, 0, 1e-9], [0, 0, 0]]]) {
    ad.add('construct', [p, q], (a, b) => direct(new AnchorPointDirect({position: c3(a), adjustmentParams: c3(b)})));
  }
  const ai = suite('AnchorPointIndirect');
  ai.add('construct', [[1, 2, 3], [4, 5, 6], [1, 0.5, 0.25, 0.5, 2, 0.125, 0.25, 0.125, 3]], (a, b, m) =>
    indirect(new AnchorPointIndirect({position: c3(a), adjustmentParams: c3(b), covarianceMatrix: Matrix3.fromColumnMajorArray(m)})));
  const sp = suite('Spdcf');
  for (const v of [[1, 0, 0, 1], [0.5, 0.25, 10, 3600], [0.001, 0.999, 4.5, 0.5]]) {
    sp.add('construct', v, (A, alpha, beta, T) => spdcf(new Spdcf({A, alpha, beta, T})));
  }
  // Out-of-range values are debug-build DeveloperErrors.
  sp.add('construct', [0, 0, 0, 1], (A, alpha, beta, T) => spdcf(new Spdcf({A, alpha, beta, T})));
  const cg = suite('CorrelationGroup');
  cg.add('construct', [[true, false, true], [0.1, 0.2, 0.3], [[1, 0.5, 2, 10], [0.5, 0, 0, 1]]], (flags, thetas, params) =>
    group(new CorrelationGroup({groupFlags: flags, rotationThetas: c3(thetas), params: params.map(([A, alpha, beta, T]) => new Spdcf({A, alpha, beta, T}))})));
  cg.add('construct', [[false, false, false], [0, 0, 0], []], (flags, thetas) => group(new CorrelationGroup({groupFlags: flags, rotationThetas: c3(thetas), params: []})));

  const ps = suite('PpeSource');
  ps.add('constants', [], () => ({...PpeSource}));
  const pm = suite('PpeMetadata');
  for (const [min, max, source] of [[0, 10, 'SIGX'], [null, null, 'VARZ'], [-1.5, null, 'SIGR'], [null, 4, 'custom']]) {
    pm.add('construct', [min, max, source], (a, b, c) => metadata(new PpeMetadata({min: a ?? undefined, max: b ?? undefined, source: c})));
  }
  const pt = suite('PpeTexture');
  for (const [traits, noData, offset, scale, index, texCoord] of [
    [[0, 5, 'SIGZ'], 255, 0.5, 0.01, 0, 1],
    [[null, null, 'VARX'], null, null, null, 3, null],
  ]) {
    pt.add('construct', [traits, noData, offset, scale, index, texCoord], (t, n, o, s, i, c) =>
      texture(new PpeTexture({traits: new PpeMetadata({min: t[0] ?? undefined, max: t[1] ?? undefined, source: t[2]}), noData: n ?? undefined, offset: o ?? undefined, scale: s ?? undefined, index: i, texCoord: c ?? undefined})));
  }
  const mp = suite('MeshPrimitiveGpmLocal');
  mp.add('construct', [[[0, 1, 'SIGX', 0], [null, null, 'SIGY', 2]]], (list) =>
    new MeshPrimitiveGpmLocal(list.map(([min, max, source, index]) => new PpeTexture({traits: new PpeMetadata({min: min ?? undefined, max: max ?? undefined, source}), index}))).ppeTextures.map(texture));
  mp.add('construct', [[]], () => new MeshPrimitiveGpmLocal([]).ppeTextures.map(texture));

  const gl = suite('GltfGpmLocal');
  gl.add('direct', [[[1, 2, 3], [4, 5, 6]], [1, 2, 3, 4, 5, 6, 7, 8, 9]], (p, m) =>
    local(new GltfGpmLocal({storageType: 'Direct', anchorPointsDirect: [new AnchorPointDirect({position: c3(p[0]), adjustmentParams: c3(p[1])})], covarianceDirect: Matrix3.fromColumnMajorArray(m)})));
  gl.add('indirect', [[[1, 2, 3], [4, 5, 6], [1, 0, 0, 0, 1, 0, 0, 0, 1]], [[true, true, false], [0, 0.5, 1]]], (p, g) =>
    local(new GltfGpmLocal({storageType: 'Indirect', anchorPointsIndirect: [new AnchorPointIndirect({position: c3(p[0]), adjustmentParams: c3(p[1]), covarianceMatrix: Matrix3.fromColumnMajorArray(p[2])})], intraTileCorrelationGroups: [new CorrelationGroup({groupFlags: g[0], rotationThetas: c3(g[1]), params: []})]})));
  gl.add('direct', [[], [0, 0, 0, 0, 0, 0, 0, 0, 0]], (p, m) => local(new GltfGpmLocal({storageType: 'Direct', anchorPointsDirect: [], covarianceDirect: Matrix3.fromColumnMajorArray(m)})));

  const loader = suite('GltfGpmLoader');
  const documents = [
    {storageType: 'Direct', anchorPointsDirect: [{position: [1, 2, 3], adjustmentParams: [0.1, 0.2, 0.3]}, {position: [-4, 5.5, 1e7], adjustmentParams: [0, 0, 1]}], covarianceDirectUpperTriangle: [1, 2, 3, 4, 5, 6]},
    {storageType: 'Direct', anchorPointsDirect: [], covarianceDirectUpperTriangle: [0.5, 0, 0.25, 0, 0, 0.125]},
    {storageType: 'Indirect', anchorPointsIndirect: [{position: [1, 1, 1], adjustmentParams: [2, 2, 2], covarianceMatrix: [1, 0.1, 2, 0.2, 0.3, 3]}], intraTileCorrelationGroups: [{groupFlags: [true, false, true], rotationThetas: [0.1, 0.2, 0.3], params: [{A: 1, alpha: 0.5, beta: 2, T: 10}, {A: 0.25, alpha: 0, beta: 0, T: 1}]}]},
    {storageType: 'Indirect', anchorPointsIndirect: [], intraTileCorrelationGroups: []},
    {storageType: 'Indirect', anchorPointsIndirect: [{position: [1, 2], adjustmentParams: [3, 4, 5, 6], covarianceMatrix: [1, 2, 3, 4, 5]}], intraTileCorrelationGroups: [{groupFlags: [], rotationThetas: [9], params: [{A: 0.5}]}]},
    {storageType: 'Direct', anchorPointsDirect: [{position: [1, 2, 3], adjustmentParams: [4, 5, 6]}], covarianceDirectUpperTriangle: [1, 2, 3, 4, 5, 6], anchorPointsIndirect: [{position: [1, 2, 3]}]},
    // Short arrays read undefined elements; the Cartesians and matrices hold NaN.
    {storageType: 'Indirect', anchorPointsIndirect: [{position: [1, 2], adjustmentParams: [3, 4, 5, 6], covarianceMatrix: [1, 2, 3, 4, 5]}], intraTileCorrelationGroups: [{groupFlags: [], rotationThetas: [9], params: []}]},
    {storageType: 'direct'},
    {},
    {storageType: 7},
    {storageType: 'Direct', anchorPointsDirect: [{adjustmentParams: [1, 2, 3]}], covarianceDirectUpperTriangle: [1, 2, 3, 4, 5, 6]},
    {storageType: 'Indirect', anchorPointsIndirect: [], intraTileCorrelationGroups: [{groupFlags: [true], rotationThetas: [1, 2, 3]}]},
    {storageType: 'Direct', anchorPointsDirect: [], covarianceDirectUpperTriangle: null},
  ];
  for (const document of documents) loader.add('load', [document], (d) => local(GltfGpmLoader.load(d)));
  loader.add('loadDirect', [documents[0]], (d) => local(GltfGpmLoader.loadDirect(d)));
  loader.add('loadIndirect', [documents[2]], (d) => local(GltfGpmLoader.loadIndirect(d)));
  return [ad, ai, sp, cg, ps, pm, pt, mp, gl, loader];
}
