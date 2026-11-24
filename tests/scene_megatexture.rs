use cesium_rs::scene::megatexture::get_3d_texture_dimension;
use serde_json::Value;

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
