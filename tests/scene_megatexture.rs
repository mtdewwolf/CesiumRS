use cesium_rs::{
    core::{webgl_constants as gl, Cartesian3, RuntimeError, TypedArray},
    renderer::{Context, Gl, GlValue},
    scene::{
        megatexture::{get_3d_texture_dimension, Megatexture},
        MetadataComponentType,
    },
};
use serde_json::Value;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct Recording3DGl {
    next: Cell<u32>,
    uploads: RefCell<Vec<Vec<u8>>>,
}

impl Gl for Recording3DGl {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([16, 16])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if let ("texSubImage3D", [.., GlValue::Pixels { bytes, .. }]) = (name, args) {
            self.uploads.borrow_mut().push(bytes.clone());
        }
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        Ok(match (name, args) {
            ("getParameter", [GlValue::Number(p)]) => match *p as u32 {
                gl::MAX_VIEWPORT_DIMS | gl::ALIASED_LINE_WIDTH_RANGE | gl::ALIASED_POINT_SIZE_RANGE => {
                    GlValue::Numbers(vec![1.0, 256.0])
                }
                gl::MAX_3D_TEXTURE_SIZE => GlValue::Number(256.0),
                gl::MAX_TEXTURE_SIZE => GlValue::Number(256.0),
                _ => GlValue::Number(32.0),
            },
            ("getExtension", _) => GlValue::Object(0),
            _ => GlValue::Null,
        })
    }
}

#[test]
fn dimensions_match_pinned_cesium() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("data/scene-megatexture.json")).unwrap();
    for case in cases {
        let number = |key: &str| case[key].as_u64().unwrap();
        let dimensions = std::array::from_fn(|i| case["dimensions"][i].as_u64().unwrap() as u32);
        let result = get_3d_texture_dimension(
            dimensions,
            number("bytes") as u32,
            number("memory"),
            case["count"].as_u64(),
            number("max") as u32,
        );
        if let Some(error) = case["error"].as_str() {
            assert_eq!(result.unwrap_err().message, error, "{case}");
        } else {
            let expected = std::array::from_fn(|i| case["result"][i].as_u64().unwrap() as u32);
            assert_eq!(result.unwrap(), expected, "{case}");
        }
    }
}

#[test]
fn float64_upload_converts_to_f32_and_preserves_slot_invariants() {
    let context = Context::new_webgl2(Recording3DGl::default());
    let mut mega = Megatexture::new(
        &context,
        Cartesian3::new(2.0, 2.0, 2.0),
        1,
        MetadataComponentType::Float64,
        1024 * 1024,
        Some(2),
    )
    .unwrap();

    // Wrong length upload must fail without mutating slot lists or occupied_count.
    assert!(mega.add(&TypedArray::Float64(vec![1.0, 2.0])).is_err());
    assert_eq!(mega.occupied_count, 0);
    assert!(mega.occupied_indices().is_empty());

    // Valid Float64 tile (8 voxels) converts to 8 * 4 = 32 bytes for PixelDatatype::Float.
    let values = vec![0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5];
    let slot = mega.add(&TypedArray::Float64(values.clone())).unwrap();
    assert_eq!(slot, 0);
    assert_eq!(mega.occupied_count, 1);
    let uploaded = context.host().uploads.borrow();
    let expected: Vec<u8> = values.iter().flat_map(|&v| (v as f32).to_le_bytes()).collect();
    assert_eq!(uploaded.last().unwrap(), &expected);
    drop(uploaded);

    // Removing once succeeds; removing the same slot again fails without underflow.
    mega.remove(slot).unwrap();
    assert_eq!(mega.occupied_count, 0);
    assert!(mega.remove(slot).is_err());
    assert_eq!(mega.occupied_count, 0);
    assert!(mega.occupied_indices().is_empty());
    mega.destroy().unwrap();
}
