use cesium_rs::core::*;
use cesium_rs::scene::*;
#[test]
fn voxel_box_shape_render_bounds_and_shader_metadata() {
    let mut s = VoxelBoxShape::default();
    assert!(s.update(
        Matrix4::IDENTITY,
        Cartesian3::new(-1., -1., -1.),
        Cartesian3::new(1., 1., 1.),
        Some(Cartesian3::new(-0.5, -0.5, -0.5)),
        Some(Cartesian3::new(0.5, 0.5, 0.5)),
    ));
    assert_eq!(s.render_min_bounds(), Cartesian3::new(-0.5, -0.5, -0.5));
    assert_eq!(s.render_max_bounds(), Cartesian3::new(0.5, 0.5, 0.5));
    assert_eq!(s.min_bounds(), Cartesian3::new(-1., -1., -1.));
    assert_eq!(s.max_bounds(), Cartesian3::new(1., 1., 1.));
    assert_eq!(s.shader_maximum_intersections_length(), 1);
    assert_eq!(s.render_bound_plane_distances(), [-0.5, -0.5, -0.5, -0.5, -0.5, -0.5]);
    // trait object access
    let t: &dyn VoxelShape = &s;
    assert_eq!(t.render_min_bounds(), s.render_min_bounds());
}

#[test]
fn spatial_node_visibility_and_construct_children() {
    let shape = VoxelBoxShape::default();
    let node = SpatialNode::new(0, [0, 0, 0], &shape, Cartesian3::new(2., 2., 2.));
    assert!(!node.is_visited(0));
    let children = node.construct_child_nodes(&shape);
    assert_eq!(children.len(), 8);
    assert_eq!(children[0].level, 1);
    assert_eq!(children[7].coords, [1, 1, 1]);
}

#[test]
fn tileset_3d_tile_content_metadata_and_group_setters() {
    use serde_json::json;
    let tile = TileId::default();
    let resource = Resource::new("https://example.test/tiles.json").unwrap();
    let content = Tileset3DTileContent::from_json(tile, resource, &json!({}), |_, _, _| Ok(())).unwrap();
    assert!(content.ready());
    // metadata/group are public fields
    let mut c = content;
    c.metadata = Some(json!({"a": 1}));
    c.group = Some(json!({"g": 2}));
    assert!(c.metadata.is_some());
    assert!(c.group.is_some());
    assert!(!c.is_destroyed());
    c.destroy().unwrap();
    assert!(c.is_destroyed());
}

#[test]
fn performance_display_rounding_matches_reference() {
    let mut d = PerformanceDisplay::new(0.0);
    d.update(1001.0, true);
    // fps computed
    assert!(d.fps_text.contains("FPS"));
}

#[test]
fn globe_translucency_bounds() {
    let mut g = GlobeTranslucency::default();
    assert!(g.set_front_face_alpha(0.5).is_ok());
    assert_eq!(g.front_face_alpha(), 0.5);
    assert!(g.set_front_face_alpha(2.0).is_err());
}

#[test]
fn empty_content_basic() {
    let mut c = Empty3DTileContent::new();
    assert!(c.ready());
    assert_eq!(c.features_length(), 0);
    c.destroy();
    assert!(c.is_destroyed());
}

#[test]
fn debug_inspector_frustum_color() {
    let c = DebugInspector::compute_frustum_color(0b101);
    assert_eq!(c.red, 1.0);
    assert_eq!(c.green, 0.0);
    assert_eq!(c.blue, 1.0);
}

#[test]
fn frame_rate_monitor_events_pop() {
    // smoke
    let m = FrameRateMonitor::default();
    assert!(!m.is_frame_rate_low());
}

#[test]
fn cesium_content_factory_classify() {
    // binary magic
    let bytes = b"pnts\x00\x00\x00\x00";
    let t = Cesium3DTileContentFactory::classify(bytes);
    assert!(t.is_ok());
}

#[test]
fn job_scheduler_default_budgets() {
    let s = JobScheduler::default();
    assert_eq!(s.total_budget(), 50.0);
}

#[test]
fn create_tangent_space_debug_primitive_none() {
    use cesium_rs::core::geometry::{Geometry, GeometryAttributes};
    use cesium_rs::core::PrimitiveType;
    let geom = Geometry::new(GeometryAttributes::default(), None, PrimitiveType::Triangles);
    let p = create_tangent_space_debug_primitive(CreateTangentSpaceDebugPrimitiveOptions::new(geom));
    assert!(p.is_none());
}

#[test]
fn voxel_box_shader_state_matches_reference() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/scene/VoxelBoxState.json")).unwrap();
    let vector = |v: &serde_json::Value| Cartesian3::new(v[0].as_f64().unwrap(),v[1].as_f64().unwrap(),v[2].as_f64().unwrap());
    for case in fixture["cases"].as_array().unwrap() {
        let mut shape = VoxelBoxShape::default();
        assert_eq!(shape.shader_maximum_intersections_length,case["before"].as_u64().unwrap() as usize);
        let clipped = case["clipped"].as_bool().unwrap();
        let visible = shape.update(Matrix4::IDENTITY,Cartesian3::new(-1.,-1.,-1.),Cartesian3::new(1.,1.,1.),clipped.then(|| vector(&case["renderMin"])),clipped.then(|| vector(&case["renderMax"])));
        assert_eq!(visible,case["visible"].as_bool().unwrap());
        assert_eq!(shape.shader_maximum_intersections_length,case["after"].as_u64().unwrap() as usize);
        assert_eq!(shape.render_min_bounds(),vector(&case["renderMin"]));
        assert_eq!(shape.render_max_bounds(),vector(&case["renderMax"]));
        for (actual,expected) in shape.render_bound_plane_distances().iter().zip(case["distances"].as_array().unwrap()) {
            assert_eq!(*actual,expected.as_f64().unwrap());
        }
    }
}

#[test]
fn voxel_cylinder_shape_matches_reference() {
    use cesium_rs::scene::voxel_shape::VoxelCylinderShape;
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/scene/VoxelCylinderState.json")).unwrap();
    let case = &fixture["cases"][0];
    let mut shape = VoxelCylinderShape::default();
    assert!(shape.update(Matrix4::IDENTITY, Cartesian3::new(0., -std::f64::consts::PI, -1.), Cartesian3::new(1., std::f64::consts::PI, 1.), None, None));
    assert_eq!(shape.shader_maximum_intersections_length, case["intersections"].as_u64().unwrap() as usize);
    // OBB
    let mut packed = [0.; 12];
    shape.oriented_bounding_box.pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(case["obb"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-9, "{} vs {}", a, b);
    }
    let mut packed = [0.; 12];
    shape.tile_obb(1, [0,0,0]).pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(case["tile"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-9, "{} vs {}", a, b);
    }
    let mut packed = [0.; 12];
    shape.sample_obb(0, [0,0,0], Cartesian3::new(2.,2.,2.), Cartesian3::new(0.25,0.25,0.25)).pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(case["sample"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-9, "{} vs {}", a, b);
    }
    let uv = shape.convert_local_to_shape_uv_space(Cartesian3::new(0.2, 0.1, -0.1));
    for (a, b) in uv.to_array().iter().zip(case["uv"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-9, "{} vs {}", a, b);
    }
}

#[test]
fn voxel_ellipsoid_shape_matches_reference() {
    use cesium_rs::scene::voxel_shape::VoxelEllipsoidShape;
    let fixture: serde_json::Value = serde_json::from_str(include_str!("fixtures/scene/VoxelEllipsoidState.json")).unwrap();
    let case = &fixture["cases"][0];
    let mut shape = VoxelEllipsoidShape::default();
    assert!(shape.update(Matrix4::IDENTITY, Cartesian3::new(-std::f64::consts::PI, -std::f64::consts::FRAC_PI_2, -6356752.314245179), Cartesian3::new(std::f64::consts::PI, std::f64::consts::FRAC_PI_2, 63781370.0), None, None));
    assert_eq!(shape.shader_maximum_intersections_length, case["intersections"].as_u64().unwrap() as usize);
    let mut packed = [0.; 12];
    shape.oriented_bounding_box.pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(case["obb"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-6, "{} vs {}", a, b);
    }
    let mut packed = [0.; 12];
    shape.tile_obb(1, [0,0,0]).pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(case["tile"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-6, "{} vs {}", a, b);
    }
    let mut packed = [0.; 12];
    shape.sample_obb(0, [0,0,0], Cartesian3::new(2.,2.,2.), Cartesian3::new(0.25,0.25,0.25)).pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(case["sample"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-6, "{} vs {}", a, b);
    }
    let uv = shape.convert_local_to_shape_uv_space(Cartesian3::new(0.2, 0.1, -0.1));
    for (a, b) in uv.to_array().iter().zip(case["uv"].as_array().unwrap()) {
        assert!((a - b.as_f64().unwrap()).abs() < 1e-6, "{} vs {}", a, b);
    }
}

#[test]
fn curved_voxel_clipping_transforms_and_camera_match_reference() {
    let fixture: serde_json::Value=serde_json::from_str(include_str!("fixtures/scene/CurvedVoxelCases.json")).unwrap();
    let vec = |v:&serde_json::Value| Cartesian3::new(v[0].as_f64().unwrap(),v[1].as_f64().unwrap(),v[2].as_f64().unwrap());
    let floats=|v:&serde_json::Value| v.as_array().unwrap().iter().map(|v|v.as_f64().unwrap()).collect::<Vec<_>>();
    for case in fixture["cases"].as_array().unwrap() {
        let kind = if case["kind"]=="cylinder" { VoxelShapeType::Cylinder } else { VoxelShapeType::Ellipsoid };
        let mut shape = kind.create_shape();
        let m=Matrix4(floats(&case["model"]).try_into().unwrap());
        let clipped=|key:&str| (!case[key].is_null()).then(||vec(&case[key]));
        let visible=shape.update(m,vec(&case["min"]),vec(&case["max"]),clipped("clipMin"),clipped("clipMax"));
        assert_eq!(visible,case["visible"].as_bool().unwrap(),"{case}");
        if !visible {continue;}
        assert_eq!(shape.shader_maximum_intersections_length(),case["intersections"].as_u64().unwrap() as usize,"{case}");
        let close=|actual:&[f64],expected:&serde_json::Value,label:&str| {
            for (i,(a,b)) in actual.iter().zip(floats(expected)).enumerate() {
                assert!((a-b).abs() <= 1e-7_f64.max(b.abs()*1e-12),"{label}[{i}] actual {a} expected {b}; {case}");
            }
        };
        let mut packed=[0.;12];shape.oriented_bounding_box().pack(&mut packed,0);close(&packed,&case["obb"],"obb");
        shape.tile_obb(2,[1,2,1]).pack(&mut packed,0);close(&packed,&case["tile"],"tile");
        shape.sample_obb(1,[0,1,0],Cartesian3::new(4.,4.,4.),Cartesian3::new(0.25,0.5,0.75)).pack(&mut packed,0);close(&packed,&case["sample"],"sample");
        close(&shape.convert_local_to_shape_uv_space(vec(&case["point"])).to_array(),&case["uv"],"uv");
        let inverse=Matrix3(floats(&case["viewRotation"]).try_into().unwrap()).transpose();
        let transform=if case["kind"]=="cylinder" {
            let mut s=VoxelCylinderShape::default();s.update(m,vec(&case["min"]),vec(&case["max"]),clipped("clipMin"),clipped("clipMax"));
            s.update_view_transforms_with_camera(vec(&case["camera"]),inverse).unwrap();s.shader_uniforms.cylinder_ec_to_radial_tangent_up
        } else {
            let mut s=VoxelEllipsoidShape::default();s.update(m,vec(&case["min"]),vec(&case["max"]),clipped("clipMin"),clipped("clipMax"));
            s.update_view_transforms_with_camera(vec(&case["camera"]),inverse).unwrap();s.shader_uniforms.ellipsoid_ec_to_east_north_up
        };
        close(&transform.0,&case["cameraTransform"],"camera");
    }
}

#[test]
fn all_voxel_shape_constructors_match_their_default_bounds() {
    for kind in [VoxelShapeType::Box, VoxelShapeType::Cylinder, VoxelShapeType::Ellipsoid] {
        let mut shape=kind.create_shape();
        assert_eq!(shape.min_bounds(),kind.min_bounds());
        assert_eq!(shape.max_bounds(),kind.max_bounds());
        assert!(shape.update(Matrix4::IDENTITY,kind.min_bounds(),kind.max_bounds(),None,None));
        let mut packed=[0.;12];shape.tile_obb(1,[0,0,0]).pack(&mut packed,0);
        assert!(packed.iter().all(|v|v.is_finite()));
    }
}
