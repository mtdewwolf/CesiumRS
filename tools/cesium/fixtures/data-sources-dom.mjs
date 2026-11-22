// Deterministic canvas metrics for visualizer state tests in Node.
class Canvas {
  width = 16;
  height = 16;
  style = {};

  getContext() {
    return {
      measureText(text) {
        return {
          width: text.length * 10,
          actualBoundingBoxLeft: 0,
          actualBoundingBoxRight: text.length * 10,
          actualBoundingBoxAscent: 8,
          actualBoundingBoxDescent: 2,
        };
      },
      fillText() {},
      strokeText() {},
      fillRect() {},
      clearRect() {},
      save() {},
      restore() {},
      beginPath() {},
      arc() {},
      fill() {},
      stroke() {},
      closePath() {},
      getImageData(_x, _y, width = 1, height = 1) {
        return { width, height, data: new Uint8ClampedArray(width * height * 4) };
      },
    };
  }
}

export function installDataSourcesDom() {
  const originalDocument = globalThis.document;
  const shims = {
    document: {
      ...originalDocument,
      body: { appendChild() {}, removeChild() {} },
      defaultView: {
        getComputedStyle(element) {
          const match = /([\d.]+px)\s+(.*)/.exec(element.style.font);
          const values = {
            "font-size": match?.[1] ?? "30px",
            "font-family": match?.[2] ?? "sans-serif",
            "font-style": "normal",
            "font-weight": "normal",
            "line-height": "normal",
          };
          return { getPropertyValue: (property) => values[property] ?? "" };
        },
      },
      createElement: (tag) => (tag === "canvas" ? new Canvas() : { style: {} }),
      getElementById: () => null,
    },
    HTMLCanvasElement: Canvas,
    HTMLImageElement: class {},
    HTMLVideoElement: class {},
    ImageBitmap: class {},
    OffscreenCanvas: class {},
    Image: class {},
  };
  const saved = Object.keys(shims).map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]);
  Object.assign(globalThis, shims);
  return () => {
    for (const [key, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  };
}
