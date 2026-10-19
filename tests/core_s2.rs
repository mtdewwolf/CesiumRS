//! S2 ID/token conversions, hierarchy and ellipsoid positions against CesiumJS.
mod common;
use cesium_rs::core::*;
use common::*;
use serde_json::json;

#[test]
fn s2_cell() {
    run(
        "core",
        "S2Cell",
        |_| TIGHT,
        |f, a| match f {
            "cell" => {
                let c = S2Cell::new(s(&a[0]).parse().unwrap())?;
                let ellipsoid = match s(&a[1]) {
                    "MOON" => Ellipsoid::MOON,
                    "UNIT_SPHERE" => Ellipsoid::UNIT_SPHERE,
                    _ => Ellipsoid::WGS84,
                };
                let children = (c.level() < 30)
                    .then(|| (0..4).map(|i| c.get_child(i).unwrap().cell_id().to_string()).collect::<Vec<_>>());
                let ancestors = (c.level() > 0).then(|| {
                    (0..=c.level())
                        .map(|level| c.get_parent_at_level(level).unwrap().cell_id().to_string())
                        .collect::<Vec<_>>()
                });
                let vertices: Vec<_> = (0..4).map(|i| c.get_vertex(i, &ellipsoid).unwrap().enc()).collect();
                ok(json!([
                    c.cell_id().to_string(),
                    c.level(),
                    S2Cell::get_token_from_id(c.cell_id()),
                    c.get_parent().map(|p| p.cell_id().to_string()),
                    children,
                    ancestors,
                    c.get_center(&ellipsoid).enc(),
                    vertices
                ]))
            }
            "fromFace" => ok(S2Cell::from_face_position_level(u(&a[0]) as u8, s(&a[1]).parse().unwrap(), u(&a[2]))
                .map(|c| c.cell_id().to_string())),
            "token" => ok(json!([
                S2Cell::is_valid_token(s(&a[0])),
                S2Cell::get_id_from_token(s(&a[0])).map(|id| id.to_string()),
                S2Cell::from_token(s(&a[0])).map(|c| c.cell_id().to_string())
            ])),
            "id" => {
                let id = s(&a[0]).parse().unwrap();
                ok(json!([S2Cell::is_valid_id(id), S2Cell::get_token_from_id(id), S2Cell::get_level(id)]))
            }
            _ => None,
        },
    );
}

#[test]
fn s2_invalid_operations_are_bounded() {
    assert!(S2Cell::new(0).is_none());
    assert!(S2Cell::new(u64::MAX).is_none());
    let root = S2Cell::new(1 << 60).unwrap();
    assert!(root.get_parent().is_none());
    assert!(root.get_parent_at_level(0).is_none());
    assert!(root.get_child(4).is_none());
    let leaf = S2Cell::new(1).unwrap();
    assert!(leaf.get_child(0).is_none());
    assert!(leaf.get_parent_at_level(31).is_none());
    assert!(leaf.get_vertex(4, &Ellipsoid::WGS84).is_none());
    assert!(S2Cell::from_face_position_level(0, 0, 31).is_none());
    assert!(S2Cell::from_face_position_level(6, 0, 1).is_none());
    assert!(S2Cell::from_face_position_level(0, 4, 1).is_none());
}
