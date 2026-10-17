use cesium_rs::core::{HilbertOrder, MortonOrder};
mod common;
use common::*;

#[test]
fn morton_cesium_fixtures() {
    run(
        "core",
        "MortonOrder",
        |_| TIGHT,
        |f, a| match f {
            "encode2D" => ok(MortonOrder::encode_2d(u(&a[0]), u(&a[1]))),
            "decode2D" => ok(MortonOrder::decode_2d(u(&a[0])).map(u32::from)),
            "encode3D" => ok(MortonOrder::encode_3d(u(&a[0]), u(&a[1]), u(&a[2]))),
            "decode3D" => ok(MortonOrder::decode_3d(u(&a[0])).map(|v| v.map(u32::from))),
            _ => None,
        },
    );
}

#[test]
fn hilbert_cesium_fixtures() {
    run(
        "core",
        "HilbertOrder",
        |_| TIGHT,
        |f, a| match f {
            "encode2D" => {
                ok(HilbertOrder::encode_2d(u(&a[0]), n(&a[1]) as u64, n(&a[2]) as u64).map(|v| v.to_string()))
            }
            "decode2D" => {
                ok(HilbertOrder::decode_2d(u(&a[0]), s(&a[1]).parse().unwrap()).map(|(x, y)| [x as f64, y as f64]))
            }
            _ => None,
        },
    );
}

#[test]
fn morton_2d_matches_interleaving_and_round_trips_boundaries() {
    assert_eq!(MortonOrder::encode_2d(1, 1), Some(3));
    assert_eq!(MortonOrder::encode_2d(6, 0), Some(20));
    for (x, y) in [(0, 0), (1, 0), (0, 1), (6, 11), (u16::MAX as u32, u16::MAX as u32)] {
        let encoded = MortonOrder::encode_2d(x, y).unwrap();
        assert_eq!(MortonOrder::decode_2d(encoded), [x as u16, y as u16]);
    }
    assert_eq!(MortonOrder::encode_2d(65_536, 0), None);
}

#[test]
fn morton_3d_matches_interleaving_and_round_trips_boundaries() {
    assert_eq!(MortonOrder::encode_3d(6, 0, 0), Some(72));
    for (x, y, z) in [(0, 0, 0), (1, 2, 3), (6, 17, 91), (1023, 1023, 1023)] {
        let encoded = MortonOrder::encode_3d(x, y, z).unwrap();
        assert_eq!(MortonOrder::decode_3d(encoded), Some([x as u16, y as u16, z as u16]));
    }
    assert_eq!(MortonOrder::encode_3d(1024, 0, 0), None);
    assert_eq!(MortonOrder::decode_3d(1 << 30), None);
}

#[test]
fn hilbert_order_matches_level_one_curve_and_round_trips() {
    let level_one = [(0, 0), (0, 1), (1, 1), (1, 0)];
    for (index, &(x, y)) in level_one.iter().enumerate() {
        assert_eq!(HilbertOrder::encode_2d(1, x, y), Some(index as u128));
        assert_eq!(HilbertOrder::decode_2d(1, index as u128), Some((x, y)));
    }

    for level in 1..=10 {
        let edge = 1u64 << level;
        for (x, y) in [(0, 0), (edge - 1, edge - 1), (edge / 3, edge / 2), (edge - 1, 0)] {
            let index = HilbertOrder::encode_2d(level, x, y).unwrap();
            assert_eq!(HilbertOrder::decode_2d(level, index), Some((x, y)));
        }
    }
}

#[test]
fn hilbert_order_rejects_invalid_inputs_and_preserves_cesium_precision_quirks() {
    assert_eq!(HilbertOrder::encode_2d(0, 0, 0), None);
    assert_eq!(HilbertOrder::encode_2d(2, 4, 0), None);
    assert_eq!(HilbertOrder::decode_2d(2, 16), None);
    assert_eq!(HilbertOrder::decode_2d(33, 0), None);
    // Cesium converts the large index to Number before testing its low bits.
    let index = HilbertOrder::encode_2d(27, 134_217_727, 134_217_727).unwrap();
    assert_eq!(index, 12_009_599_006_321_322);
    assert_eq!(HilbertOrder::decode_2d(27, index), Some((134_217_727, 134_217_726)));
}
