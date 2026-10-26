// Imagery provider request construction, metadata and feature-info parsing,
// evaluated by the pinned CesiumJS engine.
//
// Image loading is stubbed on `Resource.prototype`: the real providers build
// the resource, and the fixture records the URL they would have requested.
import { suite } from "./lib.mjs";
import { DOMParser } from "@xmldom/xmldom";

const scene = async (name) => (await import(`@cesium/engine/Source/Scene/${name}.js`)).default;
const core = async (name) => (await import(`@cesium/engine/Source/Core/${name}.js`)).default;

function feature(f) {
  const position = f.position
    ? [f.position.longitude, f.position.latitude, f.position.height]
    : null;
  const data = f.data;
  const simpleData =
    typeof data === "string" || (data && typeof data === "object" && !data.nodeType && !data.ownerDocument)
      ? data
      : null;
  return {
    name: f.name ?? null,
    description: f.description ?? null,
    position,
    properties: f.properties ?? null,
    data: simpleData,
  };
}

export default async function sceneImagery() {
  const out = [];
  const add = (name) => {
    const s = suite(name);
    out.push(s);
    return s;
  };
  // Cesium's XML feature-info parser talks to the host DOM. Stand one up for
  // this suite only: later suites must see the Node globals they expect.
  const savedGlobals = {
    document: globalThis.document,
    Node: globalThis.Node,
    XMLSerializer: globalThis.XMLSerializer,
  };
  globalThis.Node ??= { ELEMENT_NODE: 1 };
  globalThis.document ??= {
    createElement(tag) {
      return {
        tagName: tag,
        children: [],
        textContent: "",
        appendChild(child) {
          this.children.push(child);
        },
        get innerHTML() {
          const escape = (text) =>
            text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/\u00a0/g, "&nbsp;");
          return this.children.map((child) => `<${child.tagName}>${escape(child.textContent)}</${child.tagName}>`).join("");
        },
      };
    },
  };
  globalThis.XMLSerializer ??= class {
    serializeToString(node) {
      return node.toString();
    }
  };
  const Resource = await core("Resource");
  const Rectangle = await core("Rectangle");
  const WebMercatorTilingScheme = await core("WebMercatorTilingScheme");
  const GeographicTilingScheme = await core("GeographicTilingScheme");
  const UrlTemplateImageryProvider = await scene("UrlTemplateImageryProvider");
  const GetFeatureInfoFormat = await scene("GetFeatureInfoFormat");
  const OpenStreetMapImageryProvider = await scene("OpenStreetMapImageryProvider");
  const MapboxImageryProvider = await scene("MapboxImageryProvider");
  const MapboxStyleImageryProvider = await scene("MapboxStyleImageryProvider");
  const SingleTileImageryProvider = await scene("SingleTileImageryProvider");

  const rect = (r) => [r.west, r.south, r.east, r.north];
  const tryGet = (getter) => {
    try {
      return getter();
    } catch (e) {
      return { throws: String(e.message ?? e), kind: e.name ?? "Error" };
    }
  };
  const metadata = (provider) => ({
    url: provider.url,
    pickFeaturesUrl: tryGet(() => provider.pickFeaturesUrl)?.throws ? null : provider.pickFeaturesUrl,
    tileWidth: provider.tileWidth,
    tileHeight: provider.tileHeight,
    minimumLevel: provider.minimumLevel,
    maximumLevel: provider.maximumLevel ?? null,
    rectangle: rect(provider.rectangle),
    tilingScheme: provider.tilingScheme.constructor.name,
    tilingSchemeRectangle: rect(provider.tilingScheme.rectangle),
    hasAlphaChannel: provider.hasAlphaChannel,
    credit: provider.credit?.html ?? null,
    discardPolicy: null,
    tileCredits: null,
    proxy: null,
  });

  const capture = [];
  const originals = {
    fetchImage: Resource.prototype.fetchImage,
    fetchJson: Resource.prototype.fetchJson,
    fetchXML: Resource.prototype.fetchXML,
    fetchText: Resource.prototype.fetchText,
    fetch: Resource.prototype.fetch,
  };
  Resource.prototype.fetchImage = function () {
    capture.push(this.url);
    return undefined;
  };
  Resource.prototype.fetchJson = function () {
    capture.push(this.url);
    return Promise.reject(new Error("fixture: no json"));
  };
  Resource.prototype.fetchXML = function () {
    capture.push(this.url);
    return Promise.reject(new Error("fixture: no xml"));
  };
  Resource.prototype.fetchText = function () {
    capture.push(this.url);
    return Promise.reject(new Error("fixture: no text"));
  };
  Resource.prototype.fetch = function () {
    capture.push(this.url);
    return Promise.reject(new Error("fixture: no fetch"));
  };

  const urlSuite = add("UrlTemplateImageryProvider");
  try {
    // ---- UrlTemplateImageryProvider: metadata ----
    {
      const s = urlSuite;
      const options = [
        ["defaults", { url: "https://example.com/{z}/{x}/{y}.png" }],
        [
          "full",
          {
            url: "https://example.com/{z}/{x}/{y}.png",
            pickFeaturesUrl: "https://example.com/pick/{z}/{x}/{y}/{i}/{j}",
            subdomains: ["a", "b"],
            credit: '<a href="https://example.com">Example</a>',
            minimumLevel: 2,
            maximumLevel: 12,
            rectangle: Rectangle.fromDegrees(-10, -20, 30, 40),
            tileWidth: 512,
            tileHeight: 128,
            hasAlphaChannel: false,
          },
        ],
        ["geographic", { url: "https://example.com/{z}/{x}/{y}.png", subdomains: "wxyz", tilingScheme: new GeographicTilingScheme() }],
        [
          "levelZeroTiles",
          {
            url: "https://example.com/{z}/{reverseY}.png",
            tilingScheme: new WebMercatorTilingScheme({ numberOfLevelZeroTilesX: 2, numberOfLevelZeroTilesY: 1 }),
          },
        ],
      ];
      for (const [name, option] of options) {
        const provider = new UrlTemplateImageryProvider(option);
        s.add("metadata", [name], () => metadata(provider));
      }
    }

    // ---- UrlTemplateImageryProvider: tile URL construction ----
    {
      const s = urlSuite;
      const providerCases = [
        {
          name: "xyz",
          options: { url: "https://example.com/{z}/{x}/{y}.png", maximumLevel: 5 },
          tiles: [
            [0, 0, 0],
            [3, 5, 4],
            [1024, 2047, 11],
          ],
        },
        {
          name: "subdomains",
          options: { url: "https://{s}.example.com/{z}/{x}/{y}.png", subdomains: ["a", "b", "c"] },
          tiles: [
            [0, 0, 0],
            [1, 0, 0],
            [2, 0, 0],
            [3, 4, 5],
          ],
        },
        {
          name: "reverse",
          options: {
            url: "https://example.com/{reverseZ}/{reverseX}/{reverseY}.png",
            maximumLevel: 7,
            tilingScheme: new GeographicTilingScheme(),
            urlSchemeZeroPadding: { "{reverseX}": "0000", "{reverseY}": "00" },
          },
          tiles: [
            [0, 0, 0],
            [3, 1, 2],
            [7, 9, 7],
          ],
        },
        {
          name: "degrees",
          options: {
            url: "https://example.com/{westDegrees}/{southDegrees}/{eastDegrees}/{northDegrees}/{width}/{height}.png",
            tileWidth: 300,
            tileHeight: 200,
            tilingScheme: new GeographicTilingScheme(),
          },
          tiles: [
            [0, 0, 0],
            [3, 2, 3],
            [-1, 0, 0],
          ],
        },
        {
          name: "projected",
          options: {
            url: "https://example.com/{westProjected},{southProjected},{eastProjected},{northProjected}.png",
          },
          tiles: [
            [0, 0, 0],
            [5, 3, 4],
          ],
        },
        {
          name: "custom",
          options: {
            url: "https://example.com/{Time}/{z}/{x}.png",
            customTags: { Time: (provider, x, y, level) => `t${x + y + level}-{x}` },
          },
          tiles: [
            [1, 2, 3],
          ],
        },
      ];
      for (const { name, options, tiles } of providerCases) {
        const provider = new UrlTemplateImageryProvider(options);
        s.add("tileUrls", [name, tiles], () => {
          capture.length = 0;
          for (const [x, y, level] of tiles) {
            provider.requestImage(x, y, level);
          }
          return capture.slice();
        });
      }
    }

    // ---- UrlTemplateImageryProvider: feature-pick URLs ----
    {
      const s = urlSuite;
      const base = "https://example.com/pick/{z}/{x}/{y}/{i}/{j}/{reverseI}/{reverseJ}/{longitudeDegrees}/{latitudeDegrees}/{longitudeProjected}/{latitudeProjected}/{format}.json";
      for (const [name, tilingScheme] of [
        ["webmercator", new WebMercatorTilingScheme()],
        ["geographic", new GeographicTilingScheme()],
      ]) {
        const provider = new UrlTemplateImageryProvider({
          url: "https://example.com/{z}/{x}/{y}.png",
          pickFeaturesUrl: base,
          tilingScheme,
          tileWidth: 256,
          tileHeight: 256,
          getFeatureInfoFormats: [new GetFeatureInfoFormat("json")],
        });
        s.add("pickUrls", [name], () => {
          capture.length = 0;
          provider.pickFeatures(1, 2, 3, 0.5, -0.25);
          return capture.slice();
        });
      }

      s.add("pickWithoutUrl", [], () => {
        const provider = new UrlTemplateImageryProvider({
          url: "https://example.com/{z}/{x}/{y}.png",
          getFeatureInfoFormats: [new GetFeatureInfoFormat("json")],
        });
        return provider.pickFeatures(1, 2, 3, 0.5, -0.25) ?? null;
      });
      s.add("pickDisabled", [], () => {
        const provider = new UrlTemplateImageryProvider({
          url: "https://example.com/{z}/{x}/{y}.png",
          pickFeaturesUrl: "https://example.com/pick/{x}",
          enablePickFeatures: false,
          getFeatureInfoFormats: [new GetFeatureInfoFormat("json")],
        });
        return provider.pickFeatures(1, 2, 3, 0.5, -0.25) ?? null;
      });
    }

    // ---- OpenStreetMapImageryProvider ----
    {
      const s = add("OpenStreetMapImageryProvider");
      const options = [
        ["defaults", {}],
        [
          "custom",
          {
            url: "https://custom.osm.org/",
            fileExtension: "jpg",
            retinaTiles: true,
            maximumLevel: 19,
            credit: '<a href="https://example.com">Custom OSM</a>',
          },
        ],
      ];
      for (const [name, opt] of options) {
        const provider = new OpenStreetMapImageryProvider(opt);
        s.add("metadata", [name], () => metadata(provider));
        s.add("tileUrls", [name], () => {
          capture.length = 0;
          provider.requestImage(1, 2, 3);
          return capture.slice();
        });
      }
      s.add("tooManyTiles", [], () => {
        try {
          new OpenStreetMapImageryProvider({ minimumLevel: 3 });
          return null;
        } catch (e) {
          return { throws: String(e.message ?? e), kind: e.name ?? "Error" };
        }
      });
    }

    // ---- MapboxImageryProvider ----
    {
      const s = add("MapboxImageryProvider");
      const options = [
        ["defaults", { mapId: "mapbox.streets", accessToken: "token123" }],
        [
          "custom",
          {
            url: "https://custom.mapbox.com/v4/",
            mapId: "custom.map",
            accessToken: "token456",
            format: ".jpg",
            minimumLevel: 1,
            maximumLevel: 10,
            credit: "Custom Mapbox Credit",
          },
        ],
      ];
      for (const [name, opt] of options) {
        const provider = new MapboxImageryProvider(opt);
        s.add("metadata", [name], () => metadata(provider));
        s.add("tileUrls", [name], () => {
          capture.length = 0;
          provider.requestImage(2, 3, 4);
          return capture.slice();
        });
      }
    }

    // ---- MapboxStyleImageryProvider ----
    {
      const s = add("MapboxStyleImageryProvider");
      const options = [
        ["defaults", { styleId: "streets-v11", accessToken: "tok-style" }],
        ["explicitFalse", { styleId: "streets-v11", accessToken: "tok-style", scaleFactor: false }],
        [
          "custom",
          {
            url: "https://custom.style.com/v1/",
            username: "custom-user",
            styleId: "outdoors-v11",
            accessToken: "tok-custom",
            tilesize: 256,
            scaleFactor: true,
            minimumLevel: 2,
            maximumLevel: 15,
            credit: "Custom Style Credit",
          },
        ],
      ];
      for (const [name, opt] of options) {
        const provider = new MapboxStyleImageryProvider(opt);
        s.add("metadata", [name], () => metadata(provider));
        s.add("tileUrls", [name], () => {
          capture.length = 0;
          provider.requestImage(2, 3, 4);
          return capture.slice();
        });
      }
    }

    // ---- SingleTileImageryProvider ----
    {
      const s = add("SingleTileImageryProvider");
      const opt = {
        url: "https://example.com/single.png",
        tileWidth: 512,
        tileHeight: 256,
        credit: "Single Tile Credit",
      };
      const provider = new SingleTileImageryProvider(opt);
      s.add("metadata", ["defaults"], () => metadata(provider));
      s.add("tileUrls", ["defaults"], () => {
        capture.length = 0;
        provider.requestImage(0, 0, 0);
        return capture.slice();
      });
    }
  } finally {
    Resource.prototype.fetchImage = originals.fetchImage;
    Resource.prototype.fetchJson = originals.fetchJson;
    Resource.prototype.fetchXML = originals.fetchXML;
    Resource.prototype.fetchText = originals.fetchText;
    Resource.prototype.fetch = originals.fetch;
  }

  // ---- pick feature fallback and parsing ----
  {
    const s = urlSuite;
    const cases = await (async () => {
      const results = [];
      // XML first, text second: the first format fails, the second resolves.
      {
        const provider = new UrlTemplateImageryProvider({
          url: "https://example.com/{z}/{x}/{y}.png",
          pickFeaturesUrl: "https://example.com/pick/{x}/{format}",
          getFeatureInfoFormats: [new GetFeatureInfoFormat("xml"), new GetFeatureInfoFormat("text")],
        });
        const captured = [];
        const originalXML = Resource.prototype.fetchXML;
        const originalText = Resource.prototype.fetchText;
        Resource.prototype.fetchXML = function () {
          captured.push(this.url);
          return Promise.reject(new Error("no xml"));
        };
        Resource.prototype.fetchText = function () {
          captured.push(this.url);
          return Promise.resolve("<html><title>Name</title><body>Description</body></html>");
        };
        try {
          const value = await provider.pickFeatures(1, 2, 3, 0.5, -0.25);
          results.push({ captured, value: value?.map(feature) ?? null });
        } finally {
          Resource.prototype.fetchXML = originalXML;
          Resource.prototype.fetchText = originalText;
        }
      }
      // All formats fail: an empty array.
      {
        const provider = new UrlTemplateImageryProvider({
          url: "https://example.com/{z}/{x}/{y}.png",
          pickFeaturesUrl: "https://example.com/pick/{x}/{format}",
          getFeatureInfoFormats: [new GetFeatureInfoFormat("json"), new GetFeatureInfoFormat("xml")],
        });
        const captured = [];
        const originalXML = Resource.prototype.fetchXML;
        const originalJson = Resource.prototype.fetchJson;
        Resource.prototype.fetchXML = function () {
          captured.push(this.url);
          return Promise.reject(new Error("no xml"));
        };
        Resource.prototype.fetchJson = function () {
          captured.push(this.url);
          return Promise.reject(new Error("no json"));
        };
        try {
          const value = await provider.pickFeatures(1, 2, 3, 0.5, -0.25);
          results.push({ captured, value: value?.map(feature) ?? null });
        } finally {
          Resource.prototype.fetchXML = originalXML;
          Resource.prototype.fetchJson = originalJson;
        }
      }
      return results;
    })();
    s.add("pickFallback", [], () => cases);
  }

  // ---- GetFeatureInfoFormat defaults ----
  {
    const s = add("GetFeatureInfoFormat");
    s.add("defaults", [], () => {
      const types = ["json", "xml", "html", "text"];
      return types.map((type) => {
        const format = new GetFeatureInfoFormat(type);
        return { type: format.type, format: format.format, hasCallback: typeof format.callback === "function" };
      });
    });

    const geoJson = {
      type: "FeatureCollection",
      features: [
        {
          type: "Feature",
          properties: { name: "Alpha", title: "A", count: 2, nested: { a: 1 }, blank: null },
          geometry: { type: "Point", coordinates: [10.5, -20.25] },
        },
        {
          type: "Feature",
          properties: { label: "no name", otherName: "second" },
          geometry: { type: "LineString", coordinates: [[0, 0], [1, 1]] },
        },
        { type: "Feature", properties: null },
      ],
    };
    s.add("json", [], () => {
      const format = new GetFeatureInfoFormat("json");
      return format.callback(geoJson)?.map(feature) ?? null;
    });

    const xmlSamples = {
      esri: `<?xml version="1.0"?><FeatureInfoResponse xmlns="http://www.esri.com/wms"><FIELDS OBJECTID="1" NAME="Alpha" VALUE="3.5"/><FIELDS OBJECTID="2" NAME="Beta" VALUE="4"/></FeatureInfoResponse>`,
      thredds: `<?xml version="1.0"?><FeatureInfoResponse xmlns="http://www.esri.com/wms"><FeatureInfo><name>Gamma</name><value>7</value><nested><inner>deep</inner></nested></FeatureInfo></FeatureInfoResponse>`,
      mapinfo: `<?xml version="1.0"?><MultiFeatureCollection xmlns="http://www.mapinfo.com/mxp"><Feature><Val ref="name"> Delta </Val><Val ref="count">5</Val><Val>ignored</Val></Feature></MultiFeatureCollection>`,
      gml: `<?xml version="1.0"?><wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs" xmlns:gml="http://www.opengis.net/gml"><gml:featureMember><layer:city xmlns:layer="urn:test"><layer:name>Epsilon</layer:name><layer:point><gml:Point><gml:coordinates>1,2</gml:coordinates></gml:Point></layer:point></layer:city></gml:featureMember></wfs:FeatureCollection>`,
      msGml: `<?xml version="1.0"?><msGMLOutput><cities_layer><city><name>Zeta</name><population>1000</population></city><city><name>Eta</name></city></cities_layer></msGMLOutput>`,
      unknown: `<?xml version="1.0"?><Something><a x="1">text &amp; more</a><b>2</b></Something>`,
      exception: `<?xml version="1.0"?><ServiceExceptionReport version="1.3.0"><ServiceException>bad request</ServiceException></ServiceExceptionReport>`,
    };
    for (const [name, xml] of Object.entries(xmlSamples)) {
      s.add("xml", [name], () => {
        const format = new GetFeatureInfoFormat("xml");
        const doc = new DOMParser().parseFromString(xml, "text/xml");
        try {
          return format.callback(doc)?.map(feature) ?? null;
        } catch (e) {
          return { throws: String(e.message ?? e), kind: e.name ?? "Error" };
        }
      });
    }

    const textSamples = {
      simple: "<html><title>Title</title><body>Hello</body></html>",
      emptyBody: "<html><body>   </body></html>",
      exception: "<ServiceExceptionReport><ServiceException>bad</ServiceException></ServiceExceptionReport>",
      plain: "just text",
      noTitle: "<html><body>No title</body></html>",
    };
    for (const [name, text] of Object.entries(textSamples)) {
      s.add("text", [name], () => {
        const format = new GetFeatureInfoFormat("text");
        return format.callback(text)?.map(feature) ?? null;
      });
    }
  }

  for (const [key, value] of Object.entries(savedGlobals)) {
    if (value === undefined) delete globalThis[key];
    else globalThis[key] = value;
  }
  return out;
}
