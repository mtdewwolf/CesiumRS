// Installs a DOMParser (and a minimal FileReader) before @cesium/engine loads,
// so GpxDataSource's module-level `parser = new DOMParser()` sees one in Node.
import { DOMParser } from "@xmldom/xmldom";

globalThis.DOMParser = DOMParser;

// Minimal DOM for GpxDataSource's `processDescription`, which uses a scratch
// <div> only to rewrite links in the generated description HTML. Fixture GPX
// descriptions contain no links, so an identity innerHTML round-trip is exact.
globalThis.document = {
  location: { href: "http://localhost/" },
  createElement: () => ({
    _innerHTML: "",
    set innerHTML(value) {
      this._innerHTML = String(value);
    },
    get innerHTML() {
      return this._innerHTML;
    },
    querySelectorAll: () => [],
  }),
};

globalThis.FileReader = class FileReader {
  constructor() {
    this.result = undefined;
    this.error = undefined;
    this._onload = null;
    this._onerror = null;
  }
  addEventListener(type, callback) {
    if (type === "load") this._onload = callback;
    else if (type === "error") this._onerror = callback;
  }
  readAsText(blob) {
    blob.text().then(
      (text) => {
        this.result = text;
        if (this._onload) this._onload();
      },
      (error) => {
        this.error = error;
        if (this._onerror) this._onerror();
      },
    );
  }
};
