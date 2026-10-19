// Spatial ordering and vertex-buffer packing, checked against CesiumJS.
import {
  AttributeCompression as A, AttributeType, Cartesian2, Cartesian3, Cartesian4,
  Color, ComponentDatatype, EncodedCartesian3 as E, HilbertOrder, MortonOrder,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

function mortonSuite() {
  const g = generators(makeRng(201));
  const s = suite("MortonOrder");
  const pairs = [[0, 0], [1, 0], [0, 1], [65535, 65535], [32768, 0], [0, 32768]];
  for (let i = 0; i < 200; i++) pairs.push([Math.floor(g.rng() * 65536), Math.floor(g.rng() * 65536)]);
  for (const p of pairs) {
    s.add("encode2D", p, MortonOrder.encode2D);
    s.add("decode2D", [MortonOrder.encode2D(...p)], MortonOrder.decode2D);
  }
  const triples = [[0, 0, 0], [1, 2, 3], [1023, 1023, 1023], [512, 0, 0], [0, 0, 512]];
  for (let i = 0; i < 200; i++) triples.push(Array.from({length: 3}, () => Math.floor(g.rng() * 1024)));
  for (const p of triples) {
    s.add("encode3D", p, MortonOrder.encode3D);
    s.add("decode3D", [MortonOrder.encode3D(...p)], MortonOrder.decode3D);
  }
  return s;
}

function hilbertSuite() {
  const g = generators(makeRng(202));
  const s = suite("HilbertOrder");
  // Decimal strings keep BigInt indices exact in JSON. Include the known
  // Number-conversion and signed-bitwise quirks at high levels.
  for (const level of [1, 2, 3, 8, 16, 26, 27, 28, 31, 32]) {
    const n = 2 ** level;
    const points = [[0, 0], [n - 1, n - 1], [n - 1, 0], [0, n - 1]];
    for (let i = 0; i < 40; i++) points.push([Math.floor(g.rng() * n), Math.floor(g.rng() * n)]);
    for (const p of points) {
      s.add("encode2D", [level, ...p], (...args) => HilbertOrder.encode2D(...args).toString());
      const index = HilbertOrder.encode2D(level, ...p).toString();
      s.add("decode2D", [level, index], (l, i) => HilbertOrder.decode2D(l, BigInt(i)));
    }
    const max = (4n ** BigInt(level) - 1n).toString();
    s.add("decode2D", [level, max], (l, i) => HilbertOrder.decode2D(l, BigInt(i)));
  }
  for(const level of [33,40,53,54,63,64,100,256,511,512,1023]) {
    const n=2**level,last=level<=53?n-1:n*(1-Number.EPSILON/2);
    const points=[[0,0],[last,last],[last,0],[0,last],[.25,.75],[1,2],[NaN,0],[0,NaN],[NaN,5]];
    for(let i=0;i<20;i++)points.push([Math.floor(g.rng()*n),Math.floor(g.rng()*n)]);
    for(const p of points) {
      s.add("encode2D",[level,...p],(...args)=>HilbertOrder.encode2D(...args).toString());
      if(level<=511){const index=HilbertOrder.encode2D(level,...p).toString();s.add("decode2D",[level,index],(l,i)=>HilbertOrder.decode2D(l,BigInt(i)));}
    }
    if(level<=511){
      const limit=1n<<BigInt(2*level);
      const indices=[limit-1n,limit/2n,limit/2n-1n,limit/2n+1n];
      for(let i=0;i<20;i++){
        let index=0n;
        for(let j=0;j<Math.ceil(2*level/32);j++)index=(index<<32n)|BigInt(Math.floor(g.rng()*2**32));
        indices.push(index%limit);
      }
      for(const index of indices)s.add("decode2D",[level,index.toString()],(l,i)=>HilbertOrder.decode2D(l,BigInt(i)));
    }
  }
  return s;
}

function compressionSuite() {
  const g = generators(makeRng(203));
  const s = suite("AttributeCompression");
  const vectors = [Cartesian3.UNIT_X, Cartesian3.UNIT_Y, Cartesian3.UNIT_Z,
    new Cartesian3(-1, 0, 0), new Cartesian3(0, -1, 0), new Cartesian3(0, 0, -1),
    new Cartesian3(-0, 0, -1)];
  for (let i = 0; i < 100; i++) vectors.push(g.unit3());
  for (const v of vectors) {
    s.add("octEncode", [v], (v) => A.octEncode(v, new Cartesian2()));
    for (const range of [255, 1023, 65535]) {
      s.add("octEncodeInRange", [v, range], (v, r) => A.octEncodeInRange(v, r, new Cartesian2()));
      const encoded = A.octEncodeInRange(v, range, new Cartesian2());
      s.add("octDecodeInRange", [encoded.x, encoded.y, range], (x, y, r) => A.octDecodeInRange(x, y, r, new Cartesian3()));
    }
    s.add("octEncodeToCartesian4", [v], (v) => A.octEncodeToCartesian4(v, new Cartesian4()));
    const bytes = A.octEncodeToCartesian4(v, new Cartesian4());
    s.add("octDecodeFromCartesian4", [bytes], (b) => A.octDecodeFromCartesian4(b, new Cartesian3()));
    const oct = A.octEncode(v, new Cartesian2());
    s.add("octPackFloat", [oct], A.octPackFloat);
    s.add("octDecode", [oct.x, oct.y], (x, y) => A.octDecode(x, y, new Cartesian3()));
    s.add("octEncodeFloat", [v], A.octEncodeFloat);
    s.add("octDecodeFloat", [A.octEncodeFloat(v)], (e) => A.octDecodeFloat(e, new Cartesian3()));
  }
  for (let i = 0; i < 80; i++) {
    const v = [g.unit3(), g.unit3(), g.unit3()];
    s.add("octPack", v, (a, b, c) => A.octPack(a, b, c, new Cartesian2()));
    s.add("octUnpack", [A.octPack(...v, new Cartesian2())], (packed) => {
      const out = [new Cartesian3(), new Cartesian3(), new Cartesian3()];
      A.octUnpack(packed, ...out);
      return out;
    });
  }
  const coordinates = [new Cartesian2(0, 0), new Cartesian2(1, 1), new Cartesian2(0.5, 0.5),
    new Cartesian2(1 / 4095, 4094 / 4095), new Cartesian2(-1, 2)];
  for (let i = 0; i < 100; i++) coordinates.push(new Cartesian2(g.rng(), g.rng()));
  for (const uv of coordinates) {
    s.add("compressTextureCoordinates", [uv], A.compressTextureCoordinates);
    s.add("decompressTextureCoordinates", [A.compressTextureCoordinates(uv)], (c) => A.decompressTextureCoordinates(c, new Cartesian2()));
  }
  for (const withHeight of [false, true]) {
    for (const length of [0, 1, 2, 32, 100]) {
      const buffer = () => Array.from({length}, () => Math.floor(g.rng() * 65536));
      s.add("zigZagDeltaDecode", [buffer(), buffer(), withHeight ? buffer() : undefined], (u, v, h) => {
        u = new Uint16Array(u); v = new Uint16Array(v); h = h && new Uint16Array(h);
        A.zigZagDeltaDecode(u, v, h);
        return [u, v, h];
      });
    }
  }
  const types = [
    ["i8", Int8Array, ComponentDatatype.BYTE, [-128, -127, -1, 0, 1, 127]],
    ["u8", Uint8Array, ComponentDatatype.UNSIGNED_BYTE, [0, 1, 127, 128, 254, 255]],
    ["i16", Int16Array, ComponentDatatype.SHORT, [-32768, -32767, -1, 0, 1, 32767]],
    ["u16", Uint16Array, ComponentDatatype.UNSIGNED_SHORT, [0, 1, 32767, 32768, 65534, 65535]],
    ["i32", Int32Array, ComponentDatatype.INT, [-2147483648, -2147483647, -1, 0, 1, 2147483647]],
    ["u32", Uint32Array, ComponentDatatype.UNSIGNED_INT, [0, 1, 2147483647, 2147483648, 4294967294, 4294967295]],
  ];
  for (const [name, Typed, datatype, edges] of types) {
    for (const type of ["SCALAR", "VEC2", "VEC3", "VEC4", "MAT2", "MAT3", "MAT4"]) {
      const components = AttributeType.getNumberOfComponents(type);
      const data = Array.from({length: 4 * components}, (_, i) => edges[i % edges.length]);
      s.add("dequantize", [name, data, components, 4], (_, values, c, count) =>
        A.dequantize(new Typed(values), datatype, type, count));
    }
    s.add("dequantize", [name, [], 3, 0], (_, values, c, count) =>
      A.dequantize(new Typed(values), datatype, "VEC3", count));
  }
  const colors = [Color.BLACK, Color.WHITE, Color.RED, new Color(0.5, 0.5, 0.5, 0.25), new Color(-1, 2, 0.3, 0.1)];
  for (let i = 0; i < 100; i++) colors.push(g.color());
  for (const c of colors) s.add("encodeRGB8", [c], A.encodeRGB8);
  for (const encoded of [0, 1, 0xffffff, 0x123456, 1000.7, -1, 4294967297]) {
    s.add("decodeRGB8", [encoded, 0.25], (e, alpha) => A.decodeRGB8(e, new Color(0, 0, 0, alpha)));
  }
  for (const values of [[], [0, 65535, 0xf800, 0x07e0, 0x001f],
    Array.from({length: 200}, () => Math.floor(g.rng() * 65536))]) {
    s.add("decodeRGB565", [values], (v) => A.decodeRGB565(new Uint16Array(v)));
  }
  return s;
}

function encodedSuite() {
  const g = generators(makeRng(204));
  const s = suite("EncodedCartesian3");
  const values = [0, -0, 1, -1, 65536, -65536, 65535.9, -65535.9, 1234567.1234567,
    Number.MIN_VALUE, -Number.MIN_VALUE, Infinity, -Infinity, NaN];
  for (let i = 0; i < 100; i++) values.push(g.num(1e9));
  for (const v of values) s.add("encode", [v], (v) => { const e = E.encode(v); return [e.high, e.low]; });
  for (let i = 0; i < 100; i++) {
    const p = g.c3(1e8);
    s.add("fromCartesian", [p], (p) => { const e = E.fromCartesian(p); return [e.high, e.low]; });
    const index = i % 4;
    const buffer = Array(12).fill(123.456);
    s.add("writeElements", [p, buffer, index], (p, b, j) => { b = b.slice(); E.writeElements(p, b, j); return b; });
    s.add("writeElementsF32", [p, buffer, index], (p, b, j) => { b = new Float32Array(b); E.writeElements(p, b, j); return b; });
  }
  return s;
}

export default function corePacking() {
  return [mortonSuite(), hilbertSuite(), compressionSuite(), encodedSuite()];
}
