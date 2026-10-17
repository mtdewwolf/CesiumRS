mod common;

use cesium_rs::core::{AttributeCompression as A, Cartesian3, Color, EncodedCartesian3 as E};
use common::*;
use serde_json::Value;

fn u16s(value: &Value) -> Vec<u16> {
    nums(value).into_iter().map(|v| v as u16).collect()
}

#[test]
fn attribute_compression() {
    run(
        "core",
        "AttributeCompression",
        |_| TIGHT,
        |f, a| match f {
            "octEncodeInRange" => ok(A::oct_encode_in_range(c3(&a[0]), n(&a[1]))),
            "octEncode" => ok(A::oct_encode(c3(&a[0]))),
            "octEncodeToCartesian4" => ok(A::oct_encode_to_cartesian4(c3(&a[0]))),
            "octDecodeInRange" => ok(A::oct_decode_in_range(n(&a[0]), n(&a[1]), n(&a[2]))),
            "octDecode" => ok(A::oct_decode(n(&a[0]), n(&a[1]))),
            "octDecodeFromCartesian4" => ok(A::oct_decode_from_cartesian4(c4(&a[0]))),
            "octPackFloat" => ok(A::oct_pack_float(c2(&a[0]))),
            "octEncodeFloat" => ok(A::oct_encode_float(c3(&a[0]))),
            "octDecodeFloat" => ok(A::oct_decode_float(n(&a[0]))),
            "octPack" => ok(A::oct_pack(c3(&a[0]), c3(&a[1]), c3(&a[2]))),
            "octUnpack" => ok(A::oct_unpack(c2(&a[0]))),
            "compressTextureCoordinates" => ok(A::compress_texture_coordinates(c2(&a[0]))),
            "decompressTextureCoordinates" => ok(A::decompress_texture_coordinates(n(&a[0]))),
            "zigZagDeltaDecode" => {
                let mut u = u16s(&a[0]);
                let mut v = u16s(&a[1]);
                let mut height = if a[2].is_null() { None } else { Some(u16s(&a[2])) };
                A::zig_zag_delta_decode(&mut u, &mut v, height.as_deref_mut()).unwrap();
                let output = |values: Vec<u16>| values.into_iter().map(u32::from).collect::<Vec<_>>();
                ok(vec![output(u).enc(), output(v).enc(), height.map(output).enc()])
            }
            "dequantize" => {
                let values = nums(&a[1]);
                let components = u(&a[2]) as usize;
                let count = u(&a[3]) as usize;
                macro_rules! decode {
                    ($ty:ty) => {{
                        let values: Vec<$ty> = values.iter().map(|&v| v as $ty).collect();
                        A::dequantize(&values, components, count)
                    }};
                }
                let result = match s(&a[0]) {
                    "i8" => decode!(i8),
                    "u8" => decode!(u8),
                    "i16" => decode!(i16),
                    "u16" => decode!(u16),
                    "i32" => decode!(i32),
                    "u32" => decode!(u32),
                    _ => return None,
                };
                ok(result.map(|values| values.into_iter().map(f64::from).collect::<Vec<_>>()))
            }
            "encodeRGB8" => ok(A::encode_rgb8(color(&a[0]))),
            "decodeRGB8" => {
                let mut result = Color::new(0.0, 0.0, 0.0, n(&a[1]));
                A::decode_rgb8_into(n(&a[0]), &mut result);
                ok(result)
            }
            "decodeRGB565" => ok(A::decode_rgb565(&u16s(&a[0])).into_iter().map(f64::from).collect::<Vec<_>>()),
            _ => None,
        },
    );
}

#[test]
fn encoded_cartesian() {
    run(
        "core",
        "EncodedCartesian3",
        |_| TIGHT,
        |f, a| match f {
            "encode" => {
                let result = E::encode(n(&a[0]));
                ok([result.high, result.low])
            }
            "fromCartesian" => {
                let result = E::from_cartesian(c3(&a[0]));
                ok([result.high, result.low])
            }
            "writeElements" => {
                let mut buffer = nums(&a[1]);
                E::write_elements(c3(&a[0]), &mut buffer, u(&a[2]) as usize).unwrap();
                ok(buffer)
            }
            "writeElementsF32" => {
                let mut buffer: Vec<f32> = nums(&a[1]).into_iter().map(|v| v as f32).collect();
                E::write_elements_f32(c3(&a[0]), &mut buffer, u(&a[2]) as usize).unwrap();
                ok(buffer.into_iter().map(f64::from).collect::<Vec<_>>())
            }
            _ => None,
        },
    );
}

#[test]
fn buffer_validation_is_atomic() {
    let mut u = [2, 4];
    let mut v = [6];
    assert!(A::zig_zag_delta_decode(&mut u, &mut v, None).is_none());
    assert_eq!(u, [2, 4]);
    assert_eq!(v, [6]);
    let mut v = [6, 8];
    let mut h = [10];
    assert!(A::zig_zag_delta_decode(&mut u, &mut v, Some(&mut h)).is_none());
    assert_eq!(u, [2, 4]);
    assert_eq!(v, [6, 8]);
    assert_eq!(h, [10]);

    let mut destination = [7.0; 8];
    assert!(E::write_elements(Cartesian3::ONE, &mut destination, 3).is_none());
    assert!(E::write_elements(Cartesian3::ONE, &mut destination, usize::MAX).is_none());
    assert_eq!(destination, [7.0; 8]);
    assert!(A::dequantize(&[1u8, 2], 3, 1).is_none());
    assert!(A::dequantize(&[1u8], 5, 1).is_none());
    assert!(A::dequantize(&[1u8], 16, usize::MAX).is_none());
}

#[test]
fn split_positions_reconstruct_with_gpu_precision() {
    let position = Cartesian3::new(6_378_137.123456, -4_810_983.234567, 4_146_266.345678);
    let mut buffer = [0.0f32; 6];
    E::write_elements_f32(position, &mut buffer, 0).unwrap();
    for (i, original) in [position.x, position.y, position.z].into_iter().enumerate() {
        let reconstructed = f64::from(buffer[i]) + f64::from(buffer[i + 3]);
        assert!((reconstructed - original).abs() < 0.004);
    }
}
