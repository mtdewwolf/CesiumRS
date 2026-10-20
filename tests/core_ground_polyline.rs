mod common;
use cesium_rs::core::*;
use common::*;
use serde_json::json;
#[test]
fn ground_polyline() {
    run(
        "core",
        "GroundPolylineGeometry",
        |_| Tol { rel: 2e-7, abs: 2e-7, vector: false },
        |_, a| {
            let mut p = GroundPolylineGeometry::new(list(&a[0], c3))?;
            p.arc_type = if u(&a[1]) == 1 { ArcType::Geodesic } else { ArcType::Rhumb };
            p.granularity = n(&a[2]);
            p.looped = a[3].as_bool().unwrap();
            p.mercator = a[4].as_bool().unwrap();
            p.ellipsoid = if a[5].as_bool().unwrap() { Ellipsoid::MOON } else { Ellipsoid::WGS84 };
            p.scene_3d_only = a[6].as_bool().unwrap();
            p.width = 3.;
            let mut packed = vec![0.; p.packed_length()];
            p.pack(&mut packed, 0).unwrap();
            let unpacked = GroundPolylineGeometry::unpack(&packed, 0).unwrap();
            let mut repacked = vec![0.; unpacked.packed_length()];
            unpacked.pack(&mut repacked, 0).unwrap();
            ok(json!([packed.enc(), repacked.enc(), geometry_enc(p.create_geometry())]))
        },
    );
}
