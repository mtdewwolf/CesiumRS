// Generates pinned CesiumJS fixture expectations for createElevationBandMaterial's
// layered-entry transfer function and height/color texture packing.
//
// The module's layering helpers are private, so this script stubs Texture.create and
// Material.fromType to observe exactly what the real pinned module produces.
//
// Usage: node tools/cesium/generate-scene-elevation-band.mjs

import { writeFileSync } from "node:fs";
import createElevationBandMaterial from "@cesium/engine/Source/Scene/createElevationBandMaterial.js";
import Material from "@cesium/engine/Source/Scene/Material.js";
import Texture from "@cesium/engine/Source/Renderer/Texture.js";
import PixelFormat from "@cesium/engine/Source/Core/PixelFormat.js";
import PixelDatatype from "@cesium/engine/Source/Renderer/PixelDatatype.js";

const enumName = (enumObject, value) =>
  Object.entries(enumObject).find(([, candidate]) => candidate === value)?.[0];

const captured = [];
const originalCreate = Texture.create;
Texture.create = (options) => {
  captured.push(options);
  return options;
};
Material.fromType = (type, uniforms) => ({ type, uniforms });

function run(layers, floatingPointTexture, webgl2) {
  captured.length = 0;
  const material = createElevationBandMaterial({
    scene: { context: { floatingPointTexture, webgl2 } },
    layers,
  });
  const [heights, colors] = captured;
  return {
    type: material.type,
    layers,
    floating_point_texture: floatingPointTexture,
    webgl2,
    height_pixel_format: enumName(PixelFormat, heights.pixelFormat),
    height_pixel_datatype: enumName(PixelDatatype, heights.pixelDatatype),
    heights: Array.from(heights.source.arrayBufferView),
    color_pixel_format: enumName(PixelFormat, colors.pixelFormat),
    color_pixel_datatype: enumName(PixelDatatype, colors.pixelDatatype),
    colors: Array.from(colors.source.arrayBufferView),
    width: heights.source.width,
  };
}

function color(red, green, blue, alpha = 1) {
  return { red, green, blue, alpha };
}
function entry(height, c) {
  return { height, color: c };
}

const cases = [
  { name: "single-entry-extends", layers: [{ entries: [entry(500, color(1, 0, 0, 1))] }] },
  {
    name: "ascending-two",
    layers: [{ entries: [entry(1000, color(0, 1, 0, 1)), entry(2000, color(0, 0, 1, 1))] }],
  },
  {
    name: "descending-two",
    layers: [{ entries: [entry(2000, color(0, 0, 1, 1)), entry(1000, color(0, 1, 0, 1))] }],
  },
  {
    name: "unsorted-four",
    layers: [
      {
        entries: [
          entry(3000, color(1, 1, 1, 1)),
          entry(1000, color(0, 0, 0, 1)),
          entry(2000, color(1, 0, 0, 1)),
          entry(1500, color(0, 1, 0, 1)),
        ],
      },
    ],
  },
  {
    name: "duplicate-heights-and-colors",
    layers: [
      {
        entries: [
          entry(1000, color(1, 0, 0, 1)),
          entry(1000, color(1, 0, 0, 1)),
          entry(2000, color(1, 0, 0, 1)),
          entry(3000, color(1, 0, 0, 1)),
        ],
      },
    ],
  },
  {
    name: "premultiplied-alpha",
    layers: [
      {
        entries: [entry(-100, color(0.8, 0.4, 0.2, 0.5)), entry(100, color(0.2, 0.6, 1.0, 0.25))],
        extendDownwards: true,
        extendUpwards: true,
      },
    ],
  },
  {
    name: "downward-only",
    layers: [{ entries: [entry(1000, color(1, 0, 0, 1)), entry(2000, color(0, 0, 1, 1))], extendDownwards: true }],
  },
  {
    name: "upward-only",
    layers: [{ entries: [entry(1000, color(1, 0, 0, 1)), entry(2000, color(0, 0, 1, 1))], extendUpwards: true }],
  },
  {
    name: "clamped-heights",
    layers: [
      {
        entries: [entry(-1e15, color(1, 1, 0, 1)), entry(1e15, color(0, 1, 1, 1))],
      },
    ],
  },
  {
    name: "two-overlapping-layers",
    layers: [
      {
        entries: [entry(0, color(1, 0, 0, 1)), entry(1000, color(0, 0, 1, 1))],
        extendDownwards: true,
        extendUpwards: true,
      },
      { entries: [entry(500, color(0, 1, 0, 0.5)), entry(750, color(0, 1, 0, 0.5))] },
    ],
  },
  {
    name: "two-disjoint-layers-gap",
    layers: [
      { entries: [entry(0, color(1, 0, 0, 1)), entry(100, color(1, 1, 0, 1))] },
      { entries: [entry(500, color(0, 1, 0, 1)), entry(600, color(0, 1, 1, 1))] },
    ],
  },
  {
    name: "three-layers",
    layers: [
      {
        entries: [entry(0, color(1, 0, 0, 1)), entry(1000, color(0, 0, 1, 1))],
        extendDownwards: true,
        extendUpwards: true,
      },
      { entries: [entry(250, color(0, 1, 0, 0.4)), entry(750, color(0, 1, 0, 0.4))] },
      { entries: [entry(400, color(1, 1, 1, 0.25))] },
    ],
  },
  {
    name: "split-equal-heights",
    layers: [
      { entries: [entry(0, color(1, 0, 0, 1)), entry(1000, color(0, 0, 1, 1))] },
      {
        entries: [
          entry(500, color(0, 1, 0, 1)),
          entry(500, color(0, 1, 0, 1)),
          entry(700, color(1, 1, 0, 1)),
        ],
      },
    ],
  },
  {
    name: "layer-above-range",
    layers: [
      { entries: [entry(0, color(1, 0, 0, 1)), entry(100, color(1, 0, 0, 1))] },
      { entries: [entry(1000, color(0, 0, 1, 1)), entry(1100, color(0, 0, 1, 1))] },
    ],
  },
  {
    name: "layer-below-range",
    layers: [
      { entries: [entry(1000, color(1, 0, 0, 1)), entry(1100, color(1, 0, 0, 1))] },
      { entries: [entry(0, color(0, 0, 1, 1)), entry(100, color(0, 0, 1, 1))] },
    ],
  },
];

const outputs = [];
for (const testCase of cases) {
  outputs.push({ name: testCase.name, ...run(testCase.layers, false, false) });
  outputs.push({ name: testCase.name, ...run(testCase.layers, true, true) });
  outputs.push({ name: testCase.name, ...run(testCase.layers, true, false) });
}
outputs.push({ name: "empty-layers", error: true, layers: [] });
writeFileSync(
  new URL("../../tests/data/scene-elevation-band.json", import.meta.url),
  JSON.stringify(outputs, null, 2) + "\n",
);
Texture.create = originalCreate;
