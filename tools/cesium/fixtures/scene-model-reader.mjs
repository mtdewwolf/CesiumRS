import ModelReader from '@cesium/engine/Source/Scene/Model/ModelReader.js';
import AttributeType from '@cesium/engine/Source/Scene/AttributeType.js';
import ComponentDatatype from '@cesium/engine/Source/Core/ComponentDatatype.js';
import IndexDatatype from '@cesium/engine/Source/Core/IndexDatatype.js';
import PrimitiveType from '@cesium/engine/Source/Core/PrimitiveType.js';
import {Cartesian2, Cartesian3, Cartesian4, Matrix4} from '@cesium/engine';
import {suite, enc} from './lib.mjs';

function toArray(ta) {
  if (!ta) return null;
  return Array.from(ta).map(enc);
}

function mockBuffer(bytes) {
  const u8 = new Uint8Array(bytes);
  return {
    sizeInBytes: u8.byteLength,
    getBufferData: function(target, byteOffset = 0) {
      const src = new Uint8Array(u8.buffer, u8.byteOffset + byteOffset, target.byteLength);
      new Uint8Array(target.buffer, target.byteOffset, target.byteLength).set(src);
    }
  };
}

export function sceneModelReader() {
  const s = suite('ModelReader');

  // 1. convertTriangleStripToTriangleIndices
  const stripIndicesU16 = new Uint16Array([0, 1, 2, 3, 4, 5]);
  s.add('convertTriangleStripToTriangleIndices', [Array.from(stripIndicesU16)], () => {
    return toArray(ModelReader.convertTriangleStripToTriangleIndices(stripIndicesU16));
  });

  const stripIndicesU32 = new Uint32Array([10, 20, 30, 40]);
  s.add('convertTriangleStripToTriangleIndices', [Array.from(stripIndicesU32)], () => {
    return toArray(ModelReader.convertTriangleStripToTriangleIndices(stripIndicesU32));
  });

  // 2. convertTriangleFanToTriangleIndices
  const fanIndices = new Uint16Array([0, 1, 2, 3, 4]);
  s.add('convertTriangleFanToTriangleIndices', [Array.from(fanIndices)], () => {
    return toArray(ModelReader.convertTriangleFanToTriangleIndices(fanIndices));
  });

  // 3. createIndexTypedArray
  for (const dt of [IndexDatatype.UNSIGNED_BYTE, IndexDatatype.UNSIGNED_SHORT, IndexDatatype.UNSIGNED_INT]) {
    s.add('createIndexTypedArray', [dt, 6], () => {
      const arr = ModelReader.createIndexTypedArray(dt, 6);
      return {
        byteLength: arr.byteLength,
        length: arr.length,
        type: arr.constructor.name,
      };
    });
  }

  // 4. transform3D
  const points = new Float32Array([1, 2, 3, 4, 5, 6, -1, -2, -3]);
  const matrix = Matrix4.fromTranslation(new Cartesian3(10, 20, 30), new Matrix4());
  s.add('transform3D', [Array.from(points), enc(matrix)], () => {
    return toArray(ModelReader.transform3D(points, matrix));
  });

  // 5. convertZxyToXyz
  const zxy = new Float32Array([1, 2, 3, 4, 5, 6]);
  s.add('convertZxyToXyz', [Array.from(zxy), 2], () => {
    return toArray(ModelReader.convertZxyToXyz(zxy, 2));
  });

  // 6. dequantize 1D, 2D, 3D, 4D
  const q1 = new Float32Array([0, 1, 2, 3]);
  s.add('dequantize1D', [Array.from(q1), 4, 2.5, 10.0], () => {
    return toArray(ModelReader.dequantize1D(q1, 4, 2.5, 10.0));
  });

  const q2 = new Float32Array([0, 1, 2, 3]);
  const step2 = new Cartesian2(0.5, 1.5);
  const offset2 = new Cartesian2(10, 20);
  s.add('dequantize2D', [Array.from(q2), 2, enc(step2), enc(offset2)], () => {
    return toArray(ModelReader.dequantize2D(q2, 2, step2, offset2));
  });

  const q3 = new Float32Array([0, 1, 2, 3, 4, 5]);
  const step3 = new Cartesian3(0.1, 0.2, 0.3);
  const offset3 = new Cartesian3(-1, 0, 1);
  s.add('dequantize3D', [Array.from(q3), 2, enc(step3), enc(offset3)], () => {
    return toArray(ModelReader.dequantize3D(q3, 2, step3, offset3));
  });

  const q4 = new Float32Array([0, 1, 2, 3, 4, 5, 6, 7]);
  const step4 = new Cartesian4(1, 2, 3, 4);
  const offset4 = new Cartesian4(10, 20, 30, 40);
  s.add('dequantize4D', [Array.from(q4), 2, enc(step4), enc(offset4)], () => {
    return toArray(ModelReader.dequantize4D(q4, 2, step4, offset4));
  });

  // 7. octDecode (known upstream bug: passes c instead of c.x, c.y)
  const octEncoded = new Float32Array([128, 128, 0, 0, 255, 128]);
  s.add('octDecode', [Array.from(octEncoded), 2, 255], () => {
    return toArray(ModelReader.octDecode(octEncoded, 2, 255));
  });

  // 8. readAttributeAsTypedArray unquantized from buffer
  const rawFloats = new Float32Array([1, 2, 3, 4, 5, 6]);
  const attrPlain = {
    count: 2,
    type: AttributeType.VEC3,
    componentDatatype: ComponentDatatype.FLOAT,
    byteOffset: 0,
    byteStride: 12,
    normalized: false,
    buffer: mockBuffer(rawFloats.buffer),
  };
  s.add('readAttributeAsTypedArray', [{desc: 'plainBuffer'}], () => {
    return toArray(ModelReader.readAttributeAsTypedArray(attrPlain));
  });

  // 9. readAttributeAsTypedArray normalized uint8
  const rawBytes = new Uint8Array([0, 128, 255, 255, 0, 128]);
  const attrNormalized = {
    count: 2,
    type: AttributeType.VEC3,
    componentDatatype: ComponentDatatype.UNSIGNED_BYTE,
    byteOffset: 0,
    byteStride: 3,
    normalized: true,
    buffer: mockBuffer(rawBytes.buffer),
  };
  s.add('readAttributeAsTypedArray', [{desc: 'normalizedU8'}], () => {
    return toArray(ModelReader.readAttributeAsTypedArray(attrNormalized));
  });

  // 10. readAttributeAsTypedArray with dequantization
  const quantizedFloats = new Float32Array([1, 2, 3, 4, 5, 6]);
  const attrQuantized3D = {
    count: 2,
    type: AttributeType.VEC3,
    componentDatatype: ComponentDatatype.FLOAT,
    byteOffset: 0,
    byteStride: 12,
    normalized: false,
    buffer: mockBuffer(quantizedFloats.buffer),
    quantization: {
      componentDatatype: ComponentDatatype.FLOAT,
      octEncoded: false,
      quantizedVolumeStepSize: new Cartesian3(0.5, 0.5, 0.5),
      quantizedVolumeOffset: new Cartesian3(1, 2, 3),
    },
  };
  s.add('readAttributeAsTypedArray', [{desc: 'quantized3D'}], () => {
    return toArray(ModelReader.readAttributeAsTypedArray(attrQuantized3D));
  });

  // 11. readAttributeAsTypedArray interleaved buffer
  // Interleaved layout: float32 pos (3 floats = 12 bytes) + float32 uv (2 floats = 8 bytes) = 20 byte stride
  const interleavedData = new Float32Array([
    10, 20, 30, 0.1, 0.2, // vertex 0
    40, 50, 60, 0.3, 0.4, // vertex 1
  ]);
  const attrInterleavedPos = {
    count: 2,
    type: AttributeType.VEC3,
    componentDatatype: ComponentDatatype.FLOAT,
    byteOffset: 0,
    byteStride: 20,
    normalized: false,
    buffer: mockBuffer(interleavedData.buffer),
  };
  const attrInterleavedUv = {
    count: 2,
    type: AttributeType.VEC2,
    componentDatatype: ComponentDatatype.FLOAT,
    byteOffset: 12,
    byteStride: 20,
    normalized: false,
    buffer: mockBuffer(interleavedData.buffer),
  };
  s.add('readAttributeAsTypedArray', [{desc: 'interleavedPos'}], () => {
    return toArray(ModelReader.readAttributeAsTypedArray(attrInterleavedPos));
  });
  s.add('readAttributeAsTypedArray', [{desc: 'interleavedUv'}], () => {
    return toArray(ModelReader.readAttributeAsTypedArray(attrInterleavedUv));
  });

  // 12. readIndicesAsTriangleIndicesTypedArray
  const triIndices = {
    count: 3,
    typedArray: new Uint16Array([0, 1, 2]),
    indexDatatype: IndexDatatype.UNSIGNED_SHORT,
  };
  s.add('readIndicesAsTriangleIndicesTypedArray', [{mode: 'triangles'}], () => {
    return toArray(ModelReader.readIndicesAsTriangleIndicesTypedArray(triIndices, PrimitiveType.TRIANGLES));
  });

  const stripIndices = {
    count: 4,
    typedArray: new Uint16Array([0, 1, 2, 3]),
    indexDatatype: IndexDatatype.UNSIGNED_SHORT,
  };
  s.add('readIndicesAsTriangleIndicesTypedArray', [{mode: 'strip'}], () => {
    return toArray(ModelReader.readIndicesAsTriangleIndicesTypedArray(stripIndices, PrimitiveType.TRIANGLE_STRIP));
  });

  const fanIndicesObj = {
    count: 4,
    typedArray: new Uint16Array([0, 1, 2, 3]),
    indexDatatype: IndexDatatype.UNSIGNED_SHORT,
  };
  s.add('readIndicesAsTriangleIndicesTypedArray', [{mode: 'fan'}], () => {
    return toArray(ModelReader.readIndicesAsTriangleIndicesTypedArray(fanIndicesObj, PrimitiveType.TRIANGLE_FAN));
  });

  return s;
}
