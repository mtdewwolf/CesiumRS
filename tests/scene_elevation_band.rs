use cesium_rs::{
    core::{Color, PixelDatatype, PixelFormat},
    scene::{elevation_band_texture_data, ElevationBandEntry, ElevationBandLayer},
};
use serde_json::Value;

fn pixel_format(name: &str) -> PixelFormat {
    match name {
        "RGBA" => PixelFormat::Rgba,
        "RED" => PixelFormat::Red,
        "LUMINANCE" => PixelFormat::Luminance,
        other => panic!("unexpected format {other}"),
    }
}

fn pixel_datatype(name: &str) -> PixelDatatype {
    match name {
        "UNSIGNED_BYTE" => PixelDatatype::UnsignedByte,
        "FLOAT" => PixelDatatype::Float,
        other => panic!("unexpected datatype {other}"),
    }
}

fn layer(value: &Value) -> ElevationBandLayer {
    ElevationBandLayer {
        entries: value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let color = &entry["color"];
                ElevationBandEntry {
                    height: entry["height"].as_f64().unwrap(),
                    color: Color::new(
                        color["red"].as_f64().unwrap(),
                        color["green"].as_f64().unwrap(),
                        color["blue"].as_f64().unwrap(),
                        color["alpha"].as_f64().unwrap(),
                    ),
                }
            })
            .collect(),
        extend_downwards: value["extendDownwards"].as_bool().unwrap_or(false),
        extend_upwards: value["extendUpwards"].as_bool().unwrap_or(false),
    }
}

#[test]
fn layered_entries_and_textures_match_pinned_cesium() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("data/scene-elevation-band.json")).unwrap();
    for case in cases {
        let layers: Vec<_> = case["layers"].as_array().unwrap().iter().map(layer).collect();
        let result = elevation_band_texture_data(
            &layers,
            case["floating_point_texture"].as_bool().unwrap_or(false),
            case["webgl2"].as_bool().unwrap_or(false),
        );
        if case["error"].as_bool().unwrap_or(false) {
            assert!(result.is_err(), "{case}");
            continue;
        }
        let data = result.unwrap();
        assert_eq!(data.height_pixel_format, pixel_format(case["height_pixel_format"].as_str().unwrap()), "{case}");
        assert_eq!(
            data.height_pixel_datatype,
            pixel_datatype(case["height_pixel_datatype"].as_str().unwrap()),
            "{case}"
        );
        assert_eq!(data.color_bytes.len(), case["colors"].as_array().unwrap().len(), "{case}");
        let expected_colors: Vec<u8> =
            case["colors"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
        assert_eq!(data.color_bytes, expected_colors, "{case}");

        let expected_heights = case["heights"].as_array().unwrap();
        if data.height_pixel_datatype == PixelDatatype::UnsignedByte {
            let bytes: Vec<u8> = expected_heights.iter().map(|v| v.as_u64().unwrap() as u8).collect();
            assert_eq!(data.height_bytes, bytes, "{case}");
        } else {
            assert_eq!(data.height_bytes.len(), expected_heights.len() * 4, "{case}");
            for (i, value) in expected_heights.iter().enumerate() {
                let expected = value.as_f64().unwrap() as f32;
                let actual = f32::from_le_bytes(data.height_bytes[i * 4..i * 4 + 4].try_into().unwrap());
                assert_eq!(actual, expected, "{case}");
            }
        }
    }
}

#[test]
fn rejects_empty_and_missing_height_entries() {
    assert!(elevation_band_texture_data(&[], false, false).is_err());
    let mut layer = ElevationBandLayer { entries: vec![ElevationBandEntry { height: f64::NAN, color: Color::RED }], ..Default::default() };
    assert!(elevation_band_texture_data(std::slice::from_ref(&layer), false, false).is_err());
    layer.entries[0].height = 100.0;
    assert!(elevation_band_texture_data(&[layer], false, false).is_ok());
}
