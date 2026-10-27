// Generates pinned-source fixtures for ImageryLayer and ImageryLayerCollection.
//
//   node tools/cesium/generate-scene-imagery-layers.mjs
//

import { mkdirSync, writeFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import ImageryLayer from "./node_modules/@cesium/engine/Source/Scene/ImageryLayer.js";
import ImageryLayerCollection from "./node_modules/@cesium/engine/Source/Scene/ImageryLayerCollection.js";
import Rectangle from "./node_modules/@cesium/engine/Source/Core/Rectangle.js";
import Color from "./node_modules/@cesium/engine/Source/Core/Color.js";
import GeographicTilingScheme from "./node_modules/@cesium/engine/Source/Core/GeographicTilingScheme.js";
import WebMercatorTilingScheme from "./node_modules/@cesium/engine/Source/Core/WebMercatorTilingScheme.js";
import GeographicProjection from "./node_modules/@cesium/engine/Source/Core/GeographicProjection.js";
import SplitDirection from "./node_modules/@cesium/engine/Source/Scene/SplitDirection.js";
import TextureMinificationFilter from "./node_modules/@cesium/engine/Source/Renderer/TextureMinificationFilter.js";
import TextureMagnificationFilter from "./node_modules/@cesium/engine/Source/Renderer/TextureMagnificationFilter.js";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "../../tests/fixtures/scene");
const version = JSON.parse(readFileSync(join(here, "./node_modules/@cesium/engine/package.json"), "utf8")).version;

function writeSuite(name, cases) {
  mkdirSync(out, { recursive: true });
  const file = join(out, `${name}.json`);
  writeFileSync(file, JSON.stringify({ module: name, cesium: version, cases }) + "\n");
  console.log(`${name}: ${cases.length} cases`);
}

// ---- ImageryLayer ----
{
  const cases = [];
  const add = (f, a, r) => cases.push({ f, a, r });

  // 1. defaults
  {
    const l = new ImageryLayer(undefined);
    add("defaults", [], {
      brightness: l.brightness,
      contrast: l.contrast,
      hue: l.hue,
      saturation: l.saturation,
      gamma: l.gamma,
      splitDirection: l.splitDirection,
      minificationFilter: l.minificationFilter,
      magnificationFilter: l.magnificationFilter,
      show: l.show,
      alpha: l.alpha,
      nightAlpha: l.nightAlpha,
      dayAlpha: l.dayAlpha,
      colorToAlphaThreshold: l.colorToAlphaThreshold,
      rectangle: [l.rectangle.west, l.rectangle.south, l.rectangle.east, l.rectangle.north],
      isBaseLayer: l.isBaseLayer(),
    });
  }

  // 2. options
  const optionConfigs = [
    { alpha: 0.5, brightness: 1.5, show: true },
    { alpha: 0.8, nightAlpha: 0.3, dayAlpha: 0.9, contrast: 0.7, hue: 1.1, saturation: 1.4, gamma: 0.9, show: false },
    {
      rectangle: [-1.0, -0.5, 1.0, 0.5],
      cutoutRectangle: [-0.2, -0.1, 0.2, 0.1],
      colorToAlphaThreshold: 0.02,
      minimumTerrainLevel: 2,
      maximumTerrainLevel: 12,
      splitDirection: SplitDirection.RIGHT,
      minificationFilter: TextureMinificationFilter.NEAREST,
      magnificationFilter: TextureMagnificationFilter.NEAREST,
    },
    {
      colorToAlpha: [1.0, 0.0, 0.0, 1.0],
      splitDirection: SplitDirection.LEFT,
    },
  ];

  for (const opt of optionConfigs) {
    const fullOpt = {
      ...opt,
      rectangle: opt.rectangle ? new Rectangle(...opt.rectangle) : undefined,
      cutoutRectangle: opt.cutoutRectangle ? new Rectangle(...opt.cutoutRectangle) : undefined,
      colorToAlpha: opt.colorToAlpha ? Color.unpack(opt.colorToAlpha) : undefined,
    };
    const l = new ImageryLayer(undefined, fullOpt);
    add("options", [opt], {
      alpha: l.alpha,
      nightAlpha: l.nightAlpha,
      dayAlpha: l.dayAlpha,
      brightness: l.brightness,
      contrast: l.contrast,
      hue: l.hue,
      saturation: l.saturation,
      gamma: l.gamma,
      show: l.show,
      splitDirection: l.splitDirection,
      minificationFilter: l.minificationFilter,
      magnificationFilter: l.magnificationFilter,
      colorToAlphaThreshold: l.colorToAlphaThreshold,
      rectangle: [l.rectangle.west, l.rectangle.south, l.rectangle.east, l.rectangle.north],
      cutoutRectangle: l.cutoutRectangle
        ? [l.cutoutRectangle.west, l.cutoutRectangle.south, l.cutoutRectangle.east, l.cutoutRectangle.north]
        : null,
      colorToAlpha: l.colorToAlpha ? [l.colorToAlpha.red, l.colorToAlpha.green, l.colorToAlpha.blue, l.colorToAlpha.alpha] : null,
      minimumTerrainLevel: l._minimumTerrainLevel ?? null,
      maximumTerrainLevel: l._maximumTerrainLevel ?? null,
    });
  }

  // 3. getImageryRectangle
  const rectCases = [
    [[-Math.PI, -Math.PI / 2, Math.PI, Math.PI / 2], [-Math.PI, -Math.PI / 2, Math.PI, Math.PI / 2]],
    [[-Math.PI, -Math.PI / 2, Math.PI, Math.PI / 2], [0, 0, 1, 1]],
    [[-1, -1, 1, 1], [0, 0, 2, 2]],
    [[-2, -2, -1, -1], [1, 1, 2, 2]],
  ];
  for (const [pRect, lRect] of rectCases) {
    const mockProvider = {
      rectangle: new Rectangle(...pRect),
      tilingScheme: new GeographicTilingScheme(),
    };
    const l = new ImageryLayer(mockProvider, { rectangle: new Rectangle(...lRect) });
    const r = l.getImageryRectangle();
    add("getImageryRectangle", [pRect, lRect], r ? [r.west, r.south, r.east, r.north] : null);
  }

  // 4. calculateTextureTranslationAndScale
  const scaleCases = [
    [[0, 0, 1, 1], [0, 0, 1, 1]],
    [[0, 0, 1, 1], [0, 0, 2, 2]],
    [[1, 1, 2, 2], [0, 0, 2, 2]],
    [[-0.5, -0.5, 0.5, 0.5], [-1, -1, 1, 1]],
    [[-0.2, 0.1, 0.4, 0.8], [-0.5, 0.0, 0.5, 1.0]],
  ];
  for (const [tRect, iRect] of scaleCases) {
    const l = new ImageryLayer(undefined);
    const tile = { rectangle: new Rectangle(...tRect) };
    const tileImagery = {
      useWebMercatorT: false,
      readyImagery: { rectangle: new Rectangle(...iRect) },
    };
    const res = l._calculateTextureTranslationAndScale(tile, tileImagery);
    add("calculateTextureTranslationAndScale", [tRect, iRect], [res.x, res.y, res.z, res.w]);
  }

  // 5. getLevelWithMaximumTexelSpacing
  const spacingCases = [
    ["geographic", 256, 100000.0, 0.0],
    ["geographic", 256, 50000.0, 0.5],
    ["geographic", 512, 10000.0, -0.8],
    ["geographic", 256, 500.0, 0.0],
    ["webMercator", 256, 100000.0, 0.0],
    ["webMercator", 256, 20000.0, 0.785],
    ["webMercator", 512, 5000.0, -0.5],
  ];
  for (const [schemeKind, tileWidth, texelSpacing, lat] of spacingCases) {
    const tilingScheme = schemeKind === "geographic" ? new GeographicTilingScheme() : new WebMercatorTilingScheme();
    const provider = {
      tilingScheme,
      tileWidth,
      tileHeight: tileWidth,
      rectangle: tilingScheme.rectangle,
    };
    const l = new ImageryLayer(provider);
    const ellipsoid = tilingScheme.ellipsoid;
    const isGeographic = tilingScheme.projection instanceof GeographicProjection;
    const latitudeFactor = !isGeographic ? Math.cos(lat) : 1.0;
    const levelZeroMax = (ellipsoid.maximumRadius * tilingScheme.rectangle.width * latitudeFactor)
      / (tileWidth * tilingScheme.getNumberOfXTilesAtLevel(0));
    const twoToLevel = levelZeroMax / texelSpacing;
    const expectedLevel = Math.round(Math.log(twoToLevel) / Math.log(2)) | 0;
    add("getLevelWithMaximumTexelSpacing", [schemeKind, tileWidth, texelSpacing, lat], expectedLevel);
  }

  writeSuite("ImageryLayer", cases);
}

// ---- ImageryLayerCollection ----
{
  const cases = [];
  const add = (f, a, r) => cases.push({ f, a, r });

  // 1. collection lifecycle and operations
  {
    const col = new ImageryLayerCollection();
    const l0 = new ImageryLayer(undefined, { alpha: 0.1 });
    const l1 = new ImageryLayer(undefined, { alpha: 0.2 });
    const l2 = new ImageryLayer(undefined, { alpha: 0.3 });

    const events = [];
    col.layerAdded.addEventListener((layer, idx) => events.push(["added", layer.alpha, idx]));
    col.layerRemoved.addEventListener((layer, idx) => events.push(["removed", layer.alpha, idx]));
    col.layerMoved.addEventListener((layer, newIdx, oldIdx) => events.push(["moved", layer.alpha, newIdx, oldIdx]));
    col.layerShownOrHidden.addEventListener((layer, idx, show) => events.push(["shownOrHidden", layer.alpha, idx, show]));

    col.add(l0);
    col.add(l1);
    col.add(l2);

    const state1 = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    // Reordering
    col.raise(l0);
    const stateAfterRaise = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    col.raiseToTop(l0);
    const stateAfterRaiseToTop = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    col.lowerToBottom(l0);
    const stateAfterLowerToBottom = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    // Visibility & base layer
    l0.show = false;
    col._update();
    const stateAfterL0Hide = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    l1.show = false;
    col._update();
    const stateAfterL1Hide = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    l0.show = true;
    col._update();
    const stateAfterL0Show = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    // Contains and indexOf
    const unadded = new ImageryLayer(undefined);
    const containsCheck = [col.contains(l0), col.contains(unadded), col.indexOf(l0), col.indexOf(unadded)];

    // Remove
    const removedL1 = col.remove(l1, false);
    const stateAfterRemoveL1 = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);

    // RemoveAll
    col.removeAll(false);
    const lenAfterRemoveAll = col.length;

    add("lifecycle", [], {
      state1,
      stateAfterRaise,
      stateAfterRaiseToTop,
      stateAfterLowerToBottom,
      stateAfterL0Hide,
      stateAfterL1Hide,
      stateAfterL0Show,
      containsCheck,
      removedL1,
      stateAfterRemoveL1,
      lenAfterRemoveAll,
      events,
    });
  }

  // 2. add with index
  {
    const col = new ImageryLayerCollection();
    const lA = new ImageryLayer(undefined, { alpha: 0.1 });
    const lB = new ImageryLayer(undefined, { alpha: 0.2 });
    const lC = new ImageryLayer(undefined, { alpha: 0.3 });

    col.add(lA);
    col.add(lB, 0); // insert at front
    col.add(lC, 1); // insert in middle

    const state = col._layers.map((l) => [l.alpha, l._layerIndex, l.isBaseLayer()]);
    add("addWithIndex", [], state);
  }

  // 3. swap and bounds
  {
    const col = new ImageryLayerCollection();
    const l0 = new ImageryLayer(undefined, { alpha: 0.1 });
    const l1 = new ImageryLayer(undefined, { alpha: 0.2 });
    col.add(l0);
    col.add(l1);

    // raise top layer: should be no-op
    col.raise(l1);
    const state1 = col._layers.map((l) => l.alpha);

    // lower bottom layer: should be no-op
    col.lower(l0);
    const state2 = col._layers.map((l) => l.alpha);

    add("swapBounds", [], { state1, state2 });
  }

  writeSuite("ImageryLayerCollection", cases);
}
