// Generator for CesiumJS Label and LabelCollection test fixtures.
// Uses pinned @cesium/engine 23.0.0.
// Accurately labels genuine upstream runtime snapshots versus simulated CPU expectations.

class HTMLCanvasElement {}
globalThis.HTMLCanvasElement = HTMLCanvasElement;
globalThis.HTMLImageElement = class HTMLImageElement {};
globalThis.HTMLVideoElement = class HTMLVideoElement {};
globalThis.ImageData = class ImageData {};
globalThis.ImageBitmap = class ImageBitmap {};
globalThis.OffscreenCanvas = class OffscreenCanvas {};
globalThis.window = globalThis;
globalThis.Uint8ClampedArray = Uint8ClampedArray;
globalThis.document = {
  createElement: () => {
    const c = Object.create(HTMLCanvasElement.prototype);
    c.style = {};
    c.width = 32;
    c.height = 32;
    c.getContext = () => ({
      fillStyle: "",
      fillRect: () => {},
      fillText: () => {},
      strokeText: () => {},
      measureText: (txt) => ({
        width: 24,
        actualBoundingBoxAscent: 36,
        actualBoundingBoxDescent: 12,
        actualBoundingBoxLeft: 0,
      }),
      getImageData: (x, y, w, h) => ({ data: new Uint8ClampedArray((w || 32) * (h || 32) * 4) }),
      putImageData: () => {}
    });
    return c;
  },
  body: { appendChild: () => {}, removeChild: () => {} },
  defaultView: { getComputedStyle: () => ({ getPropertyValue: () => "30px" }) }
};

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  Label,
  LabelCollection,
  LabelStyle,
  Cartesian2,
  Cartesian3,
  Color,
  NearFarScalar,
  DistanceDisplayCondition,
  HeightReference,
  HorizontalOrigin,
  VerticalOrigin,
  BoundingSphere,
  Matrix4,
} from "@cesium/engine";

const here = dirname(fileURLToPath(import.meta.url));

const snapshot = (l) => {
  const scalar = (s) => (s ? [s.near, s.nearValue, s.far, s.farValue] : null);
  return {
    show: l.show,
    position: [l.position.x, l.position.y, l.position.z],
    text: l.text,
    font: l.font,
    style: l.style,
    scale: l.scale,
    showBackground: l.showBackground,
    backgroundColor: [
      l.backgroundColor.red,
      l.backgroundColor.green,
      l.backgroundColor.blue,
      l.backgroundColor.alpha,
    ],
    backgroundPadding: [l.backgroundPadding.x, l.backgroundPadding.y],
    pixelOffset: [l.pixelOffset.x, l.pixelOffset.y],
    eyeOffset: [l.eyeOffset.x, l.eyeOffset.y, l.eyeOffset.z],
    horizontalOrigin: l.horizontalOrigin,
    verticalOrigin: l.verticalOrigin,
    heightReference: l.heightReference,
    fillColor: [l.fillColor.red, l.fillColor.green, l.fillColor.blue, l.fillColor.alpha],
    outlineColor: [
      l.outlineColor.red,
      l.outlineColor.green,
      l.outlineColor.blue,
      l.outlineColor.alpha,
    ],
    outlineWidth: l.outlineWidth,
    scaleByDistance: scalar(l.scaleByDistance),
    translucencyByDistance: scalar(l.translucencyByDistance),
    pixelOffsetScaleByDistance: scalar(l.pixelOffsetScaleByDistance),
    distanceDisplayCondition: l.distanceDisplayCondition
      ? [l.distanceDisplayCondition.near, l.distanceDisplayCondition.far]
      : null,
    disableDepthTestDistance: l.disableDepthTestDistance ?? null,
    id: l.id ?? null,
  };
};

const c = new LabelCollection();
const defLabel = c.add();
const customLabel = c.add({
  position: new Cartesian3(131075, -65538, 3.5),
  text: "CesiumRS Label Parity",
  font: "24px Helvetica",
  style: LabelStyle.FILL_AND_OUTLINE,
  scale: 1.5,
  show: true,
  showBackground: true,
  backgroundColor: new Color(0.2, 0.3, 0.4, 0.85),
  backgroundPadding: new Cartesian2(10, 8),
  pixelOffset: new Cartesian2(15, -20),
  eyeOffset: new Cartesian3(2, 4, -10),
  horizontalOrigin: HorizontalOrigin.RIGHT,
  verticalOrigin: VerticalOrigin.BOTTOM,
  heightReference: HeightReference.NONE,
  fillColor: new Color(0.9, 0.8, 0.7, 1.0),
  outlineColor: new Color(0.1, 0.1, 0.1, 1.0),
  outlineWidth: 2.5,
  scaleByDistance: new NearFarScalar(100, 1.8, 1000, 0.4),
  translucencyByDistance: new NearFarScalar(50, 1.0, 500, 0.1),
  pixelOffsetScaleByDistance: new NearFarScalar(20, 1.2, 200, 0.6),
  distanceDisplayCondition: new DistanceDisplayCondition(10, 2000),
  disableDepthTestDistance: 150.0,
  id: "custom-label-id",
});

// Unsupported characters filtering test cases
const filterCases = [
  {
    input: "Clean String 123",
    expected: Label.filterUnsupportedCharacters("Clean String 123"),
  },
  {
    input: "With\u0000Null\u0007Bell\u0008Backspace",
    expected: Label.filterUnsupportedCharacters("With\u0000Null\u0007Bell\u0008Backspace"),
  },
  {
    input: "With\u000EShiftOut\u001FUnitSep",
    expected: Label.filterUnsupportedCharacters("With\u000EShiftOut\u001FUnitSep"),
  },
  {
    input: "Soft\u00ADHyphen\u200BZeroWidth\u200DJoiner",
    expected: Label.filterUnsupportedCharacters("Soft\u00ADHyphen\u200BZeroWidth\u200DJoiner"),
  },
  {
    input: "Preserve\tTab\nNewline\rReturn",
    expected: Label.filterUnsupportedCharacters("Preserve\tTab\nNewline\rReturn"),
  },
];

// Right-to-Left reversal test cases
Label.enableRightToLeftDetection = true;
const rtlCollection = new LabelCollection();
const rtlCases = [
  "Hello World",
  "\u05e9\u05dc\u05d5\u05dd",
  "\u05e9\u05dc\u05d5\u05dd (world)!",
  "English and \u05e2\u05d1\u05e8\u05d9\u05ea together",
  "Line 1\nLine 2 with \u05e2\u05d1\u05e8\u05d9\u05ea\n\u05e2\u05d1\u05e8\u05d9\u05ea first",
].map((text) => {
  const lbl = rtlCollection.add({ text });
  return {
    input: text,
    expectedRendered: lbl._renderedText,
  };
});
Label.enableRightToLeftDetection = false;

// Equality checks
const cloneCustom = c.add({
  position: new Cartesian3(131075, -65538, 3.5),
  text: "CesiumRS Label Parity",
  font: "24px Helvetica",
  style: LabelStyle.FILL_AND_OUTLINE,
  scale: 1.5,
  show: true,
  showBackground: true,
  backgroundColor: new Color(0.2, 0.3, 0.4, 0.85),
  backgroundPadding: new Cartesian2(10, 8),
  pixelOffset: new Cartesian2(15, -20),
  eyeOffset: new Cartesian3(2, 4, -10),
  horizontalOrigin: HorizontalOrigin.RIGHT,
  verticalOrigin: VerticalOrigin.BOTTOM,
  heightReference: HeightReference.NONE,
  fillColor: new Color(0.9, 0.8, 0.7, 1.0),
  outlineColor: new Color(0.1, 0.1, 0.1, 1.0),
  outlineWidth: 2.5,
  scaleByDistance: new NearFarScalar(100, 1.8, 1000, 0.4),
  translucencyByDistance: new NearFarScalar(50, 1.0, 500, 0.1),
  pixelOffsetScaleByDistance: new NearFarScalar(20, 1.2, 200, 0.6),
  distanceDisplayCondition: new DistanceDisplayCondition(10, 2000),
  disableDepthTestDistance: 150.0,
  id: "custom-label-id",
});

const defEqualsDef = defLabel.equals(c.get(0));
const defEqualsCustom = defLabel.equals(customLabel);
const customEqualsClone = customLabel.equals(cloneCustom);

// Collection operations
const initialLength = c.length;
const containsCustom = c.contains(customLabel);
const firstRemoval = c.remove(customLabel);
const secondRemoval = c.remove(customLabel);
const containsAfterRemoval = c.contains(customLabel);
const lengthAfterRemoval = c.length;

// Bounding sphere
const points = [new Cartesian3(131075, -65538, 3.5), new Cartesian3(10, 20, 30)];
const sphere = BoundingSphere.fromPoints(points);
const translation = Matrix4.fromTranslation(new Cartesian3(10, 20, 30));
const transformedSphere = BoundingSphere.transform(
  sphere,
  translation,
  new BoundingSphere(),
);

// CPU layout and screen-space bounding box test cases with simulated canvas metrics
// Bounding box with background billboard
const testLayoutCollection = new LabelCollection();
const layoutLabel = testLayoutCollection.add({
  text: "AB",
  showBackground: true,
  scale: 1.0,
  horizontalOrigin: HorizontalOrigin.LEFT,
  verticalOrigin: VerticalOrigin.BASELINE,
});
testLayoutCollection._backgroundBillboardCollection.update = () => {};
testLayoutCollection._glyphBillboardCollection.update = () => {};
testLayoutCollection.update({});

const bboxCases = [
  {
    name: "baseline_left_bg",
    horizontalOrigin: HorizontalOrigin.LEFT,
    verticalOrigin: VerticalOrigin.BASELINE,
    center: [100, 200],
    backgroundWidth: layoutLabel._backgroundBillboard.width,
    backgroundHeight: layoutLabel._backgroundBillboard.height,
    backgroundTranslate: [
      layoutLabel._backgroundBillboard._translate.x,
      layoutLabel._backgroundBillboard._translate.y,
    ],
    expectedBbox: (() => {
      const rect = Label.getScreenSpaceBoundingBox(layoutLabel, new Cartesian2(100, 200));
      return [rect.x, rect.y, rect.width, rect.height];
    })(),
  },
];

const output = {
  defaults: snapshot(defLabel),
  custom: snapshot(customLabel),
  filterCases,
  rtlCases,
  equality: {
    defEqualsDef,
    defEqualsCustom,
    customEqualsClone,
  },
  collectionOps: {
    initialLength,
    containsCustom,
    firstRemoval,
    secondRemoval,
    containsAfterRemoval,
    lengthAfterRemoval,
  },
  boundingSphere: {
    center: [sphere.center.x, sphere.center.y, sphere.center.z],
    radius: sphere.radius,
  },
  transformedBoundingSphere: {
    center: [
      transformedSphere.center.x,
      transformedSphere.center.y,
      transformedSphere.center.z,
    ],
    radius: transformedSphere.radius,
  },
  bboxCases,
  notes: {
    engineVersion: "23.0.0",
    simulationLabel: "Glyph metrics in bboxCases are computed via standard measureText mock (width 24, ascent 36, descent 12). WebGL texture atlas and GPU draw commands are omitted from CPU tests.",
  },
};

const targetPath = join(here, "../../tests/data/scene-labels.json");
mkdirSync(dirname(targetPath), { recursive: true });
writeFileSync(targetPath, JSON.stringify(output, null, 2) + "\n");
console.log(`Wrote ${targetPath}`);
