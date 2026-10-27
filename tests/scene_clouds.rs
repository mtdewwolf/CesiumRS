//! CPU reference tests and validation for Scene `CumulusCloud` and `CloudCollection`.

pub use cesium_rs::{core, scene};
use cesium_rs::core::*;
use cesium_rs::scene::CloudType;
use serde_json::Value;

use cesium_rs::scene::{cloud_collection, cumulus_cloud};

use cloud_collection::*;
use cumulus_cloud::*;

fn load_fixtures() -> Value {
    let data = include_str!("data/scene-clouds.json");
    serde_json::from_str(data).expect("Valid JSON scene-clouds fixture")
}

fn compare_f64_slice(actual: &[f64], expected: &[Value], eps: f64) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "Slice lengths differ: actual {}, expected {}",
        actual.len(),
        expected.len()
    );
    for (i, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
        let b_val = b.as_f64().expect("Expected float");
        assert!(
            (a - b_val).abs() <= eps,
            "Mismatch at index {}: actual {}, expected {}",
            i,
            a,
            b_val
        );
    }
}

fn compare_u8_slice(actual: &[u8], expected: &[Value]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
        let b_val = b.as_u64().expect("Expected integer") as u8;
        assert_eq!(*a, b_val, "Mismatch at index {}", i);
    }
}

#[test]
fn reference_collection_defaults_and_custom() {
    let fixture = load_fixtures();
    let defs = &fixture["collectionDefaults"];

    let mut coll = CloudCollection::new();
    assert_eq!(coll.show, defs["show"].as_bool().unwrap());
    assert_eq!(coll.noise_detail, defs["noiseDetail"].as_f64().unwrap());
    assert_eq!(coll.noise_offset.x, defs["noiseOffset"][0].as_f64().unwrap());
    assert_eq!(coll.noise_offset.y, defs["noiseOffset"][1].as_f64().unwrap());
    assert_eq!(coll.noise_offset.z, defs["noiseOffset"][2].as_f64().unwrap());
    assert_eq!(coll.debug_billboards, defs["debugBillboards"].as_bool().unwrap());
    assert_eq!(coll.debug_ellipsoids, defs["debugEllipsoids"].as_bool().unwrap());
    assert_eq!(coll.len(), 0);

    let dims = coll.noise_texture_dimensions();
    assert_eq!(dims.x, defs["noiseTextureDimensions"][0].as_f64().unwrap());
    assert_eq!(dims.y, defs["noiseTextureDimensions"][1].as_f64().unwrap());
    assert_eq!(dims.z, defs["noiseTextureDimensions"][2].as_f64().unwrap());

    let custom_opts = CloudCollectionOptions {
        show: false,
        noise_detail: 32.0,
        noise_offset: Cartesian3::new(10.0, 20.0, 30.0),
        debug_billboards: true,
        debug_ellipsoids: true,
        model_matrix: Matrix4::IDENTITY,
    };
    let coll_custom = CloudCollection::with_options(custom_opts);
    let cust = &fixture["collectionCustom"];
    assert_eq!(coll_custom.show, cust["show"].as_bool().unwrap());
    assert_eq!(coll_custom.noise_detail, cust["noiseDetail"].as_f64().unwrap());
    assert_eq!(coll_custom.noise_offset.x, cust["noiseOffset"][0].as_f64().unwrap());
    assert_eq!(coll_custom.noise_offset.y, cust["noiseOffset"][1].as_f64().unwrap());
    assert_eq!(coll_custom.noise_offset.z, cust["noiseOffset"][2].as_f64().unwrap());
    assert_eq!(coll_custom.debug_billboards, cust["debugBillboards"].as_bool().unwrap());
    assert_eq!(coll_custom.debug_ellipsoids, cust["debugEllipsoids"].as_bool().unwrap());
}

#[test]
fn reference_cloud_dimensions_and_snapshots() {
    let fixture = load_fixtures();
    let clouds_fixture = &fixture["clouds"];

    let mut coll = CloudCollection::new();

    // 1. Default cloud
    let c_def = coll.add(CumulusCloudOptions::default()).unwrap();
    {
        let c = c_def.borrow();
        let exp = &clouds_fixture["defaultCloud"];
        assert_eq!(c.show(), exp["show"].as_bool().unwrap());
        assert_eq!(c.position().x, exp["position"][0].as_f64().unwrap());
        assert_eq!(c.position().y, exp["position"][1].as_f64().unwrap());
        assert_eq!(c.position().z, exp["position"][2].as_f64().unwrap());
        assert_eq!(c.scale().x, exp["scale"][0].as_f64().unwrap());
        assert_eq!(c.scale().y, exp["scale"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().x, exp["maximumSize"][0].as_f64().unwrap());
        assert_eq!(c.maximum_size().y, exp["maximumSize"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().z, exp["maximumSize"][2].as_f64().unwrap());
        assert_eq!(c.slice(), exp["slice"].as_f64().unwrap());
        assert_eq!(c.brightness(), exp["brightness"].as_f64().unwrap());
        assert_eq!(c.color().red, exp["color"][0].as_f64().unwrap());
        assert_eq!(c.color().green, exp["color"][1].as_f64().unwrap());
        assert_eq!(c.color().blue, exp["color"][2].as_f64().unwrap());
        assert_eq!(c.color().alpha, exp["color"][3].as_f64().unwrap());
    }

    // 2. Custom cloud
    let c_cust = coll
        .add(CumulusCloudOptions {
            show: true,
            position: Cartesian3::new(131075.0, -65538.0, 3.5),
            scale: Some(Cartesian2::new(25.0, 15.0)),
            maximum_size: Some(Cartesian3::new(28.0, 16.0, 12.0)),
            slice: 0.45,
            brightness: 0.8,
            color: Color::new(0.2, 0.4, 0.6, 0.8),
            cloud_type: CloudType::Cumulus,
        })
        .unwrap();
    {
        let c = c_cust.borrow();
        let exp = &clouds_fixture["customCloud"];
        assert_eq!(c.scale().x, exp["scale"][0].as_f64().unwrap());
        assert_eq!(c.scale().y, exp["scale"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().x, exp["maximumSize"][0].as_f64().unwrap());
        assert_eq!(c.maximum_size().y, exp["maximumSize"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().z, exp["maximumSize"][2].as_f64().unwrap());
        assert_eq!(c.slice(), exp["slice"].as_f64().unwrap());
        assert_eq!(c.brightness(), exp["brightness"].as_f64().unwrap());
    }

    // 3. Only maximum size specified (scale derived as maximumSize.x, y)
    let c_only_max = coll
        .add(CumulusCloudOptions {
            maximum_size: Some(Cartesian3::new(30.0, 18.0, 10.0)),
            ..Default::default()
        })
        .unwrap();
    {
        let c = c_only_max.borrow();
        let exp = &clouds_fixture["onlyMaxCloud"];
        assert_eq!(c.scale().x, exp["scale"][0].as_f64().unwrap());
        assert_eq!(c.scale().y, exp["scale"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().x, exp["maximumSize"][0].as_f64().unwrap());
        assert_eq!(c.maximum_size().y, exp["maximumSize"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().z, exp["maximumSize"][2].as_f64().unwrap());
    }

    // 4. Only scale specified (maximumSize derived as scale.x, scale.y, min(x, y) / 1.5)
    let c_only_scale = coll
        .add(CumulusCloudOptions {
            scale: Some(Cartesian2::new(16.0, 9.0)),
            ..Default::default()
        })
        .unwrap();
    {
        let c = c_only_scale.borrow();
        let exp = &clouds_fixture["onlyScaleCloud"];
        assert_eq!(c.scale().x, exp["scale"][0].as_f64().unwrap());
        assert_eq!(c.scale().y, exp["scale"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().x, exp["maximumSize"][0].as_f64().unwrap());
        assert_eq!(c.maximum_size().y, exp["maximumSize"][1].as_f64().unwrap());
        assert_eq!(c.maximum_size().z, exp["maximumSize"][2].as_f64().unwrap());
    }
}

#[test]
fn reference_vertex_attributes_instanced_and_batched() {
    let fixture = load_fixtures();
    let attrs_fixture = &fixture["attributes"];

    let mut coll = CloudCollection::new();
    let def_handle = coll.add(CumulusCloudOptions::default()).unwrap();
    let cust_handle = coll
        .add(CumulusCloudOptions {
            show: true,
            position: Cartesian3::new(131075.0, -65538.0, 3.5),
            scale: Some(Cartesian2::new(25.0, 15.0)),
            maximum_size: Some(Cartesian3::new(28.0, 16.0, 12.0)),
            slice: 0.45,
            brightness: 0.8,
            color: Color::new(0.2, 0.4, 0.6, 0.8),
            cloud_type: CloudType::Cumulus,
        })
        .unwrap();

    let instanced = coll.instanced_attributes();
    assert_eq!(instanced.len(), 2);

    // Verify instanced attributes against CesiumJS output
    let def_exp_inst = &attrs_fixture["defaultCloud"]["instanced"];
    compare_f64_slice(
        &instanced[0].position_high_and_scale_x,
        def_exp_inst["positionHighAndScaleX"].as_array().unwrap(),
        1e-6,
    );
    compare_f64_slice(
        &instanced[0].position_low_and_scale_y,
        def_exp_inst["positionLowAndScaleY"].as_array().unwrap(),
        1e-6,
    );
    compare_f64_slice(
        &instanced[0].packed_attribute0,
        def_exp_inst["packedAttribute0"].as_array().unwrap(),
        1e-6,
    );
    compare_f64_slice(
        &instanced[0].packed_attribute1,
        def_exp_inst["packedAttribute1"].as_array().unwrap(),
        1e-6,
    );
    compare_u8_slice(&instanced[0].color, def_exp_inst["color"].as_array().unwrap());

    let cust_exp_inst = &attrs_fixture["customCloud"]["instanced"];
    compare_f64_slice(
        &instanced[1].position_high_and_scale_x,
        cust_exp_inst["positionHighAndScaleX"].as_array().unwrap(),
        1e-6,
    );
    compare_f64_slice(
        &instanced[1].position_low_and_scale_y,
        cust_exp_inst["positionLowAndScaleY"].as_array().unwrap(),
        1e-6,
    );
    compare_f64_slice(
        &instanced[1].packed_attribute0,
        cust_exp_inst["packedAttribute0"].as_array().unwrap(),
        1e-6,
    );
    compare_f64_slice(
        &instanced[1].packed_attribute1,
        cust_exp_inst["packedAttribute1"].as_array().unwrap(),
        1e-6,
    );
    compare_u8_slice(&instanced[1].color, cust_exp_inst["color"].as_array().unwrap());

    // Verify batched attributes against CesiumJS 4 quad corners
    let batched = coll.batched_attributes();
    assert_eq!(batched.len(), 2);

    let cust_exp_batched = attrs_fixture["customCloud"]["batched"].as_array().unwrap();
    for corner in 0..4 {
        let exp_vert = &cust_exp_batched[corner];
        compare_f64_slice(
            &batched[1][corner].packed_attribute0,
            exp_vert["packedAttribute0"].as_array().unwrap(),
            1e-6,
        );
        compare_f64_slice(
            &batched[1][corner].position_high_and_scale_x,
            exp_vert["positionHighAndScaleX"].as_array().unwrap(),
            1e-6,
        );
    }

    drop(def_handle);
    drop(cust_handle);
}

#[test]
fn reference_collection_lifecycle_and_compaction() {
    let fixture = load_fixtures();
    let life = &fixture["lifecycle"];

    let mut coll = CloudCollection::new();
    let def_c = coll.add(CumulusCloudOptions::default()).unwrap();
    let cust_c = coll
        .add(CumulusCloudOptions {
            show: true,
            position: Cartesian3::new(131075.0, -65538.0, 3.5),
            scale: Some(Cartesian2::new(25.0, 15.0)),
            maximum_size: Some(Cartesian3::new(28.0, 16.0, 12.0)),
            slice: 0.45,
            brightness: 0.8,
            color: Color::new(0.2, 0.4, 0.6, 0.8),
            cloud_type: CloudType::Cumulus,
        })
        .unwrap();
    let _m = coll
        .add(CumulusCloudOptions {
            maximum_size: Some(Cartesian3::new(30.0, 18.0, 10.0)),
            ..Default::default()
        })
        .unwrap();
    let _s = coll
        .add(CumulusCloudOptions {
            scale: Some(Cartesian2::new(16.0, 9.0)),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(coll.len(), life["initialLength"].as_u64().unwrap() as usize);
    assert_eq!(coll.contains(&cust_c), life["containsBefore"].as_bool().unwrap());

    // First removal returns true
    assert_eq!(coll.remove(&cust_c), life["firstRemoval"].as_bool().unwrap());
    // Second removal returns false
    assert_eq!(coll.remove(&cust_c), life["secondRemoval"].as_bool().unwrap());
    // Cloud is now detached
    assert!(!cust_c.borrow().is_attached());
    assert_eq!(coll.contains(&cust_c), life["containsAfter"].as_bool().unwrap());
    // Collection length compacts
    assert_eq!(coll.len(), life["lengthAfter"].as_u64().unwrap() as usize);

    // Retained first cloud still matches
    let retained = coll.get(0).unwrap();
    assert_eq!(retained.borrow().show(), def_c.borrow().show());
    assert_eq!(retained.borrow().scale(), def_c.borrow().scale());

    // Test removeAll and destroy
    let mut test_c = CloudCollection::new();
    let _t1 = test_c.add(CumulusCloudOptions::default()).unwrap();
    let _t2 = test_c.add(CumulusCloudOptions::default()).unwrap();
    assert_eq!(test_c.len(), life["testCollInitialLen"].as_u64().unwrap() as usize);
    test_c.remove_all();
    assert_eq!(test_c.len(), life["testCollAfterRemoveAllLen"].as_u64().unwrap() as usize);
    assert_eq!(test_c.is_destroyed(), life["isDestroyedBefore"].as_bool().unwrap());
    test_c.destroy();
    assert_eq!(test_c.is_destroyed(), life["isDestroyedAfter"].as_bool().unwrap());
    assert!(test_c.add(CumulusCloudOptions::default()).is_err());
    assert!(test_c.get(0).is_none());
}

#[test]
fn position_only_bounding_sphere_extension() {
    let fixture = load_fixtures();
    let bs = &fixture["boundingSphere"];
    let tbs = &fixture["transformedBoundingSphere"];

    let mut coll = CloudCollection::new();
    let _c1 = coll
        .add(CumulusCloudOptions {
            position: Cartesian3::new(131075.0, -65538.0, 3.5),
            ..Default::default()
        })
        .unwrap();
    let _c2 = coll
        .add(CumulusCloudOptions {
            position: Cartesian3::new(10.0, 20.0, 30.0),
            ..Default::default()
        })
        .unwrap();

    let sphere = coll.base_volume();
    assert!((sphere.center.x - bs["center"][0].as_f64().unwrap()).abs() < 1e-6);
    assert!((sphere.center.y - bs["center"][1].as_f64().unwrap()).abs() < 1e-6);
    assert!((sphere.center.z - bs["center"][2].as_f64().unwrap()).abs() < 1e-6);
    assert!((sphere.radius - bs["radius"].as_f64().unwrap()).abs() < 1e-6);

    coll.model_matrix = Matrix4::from_translation(Cartesian3::new(25.0, 50.0, 75.0));
    let sphere_wc = coll.base_volume_wc();
    assert!((sphere_wc.center.x - tbs["center"][0].as_f64().unwrap()).abs() < 1e-6);
    assert!((sphere_wc.center.y - tbs["center"][1].as_f64().unwrap()).abs() < 1e-6);
    assert!((sphere_wc.center.z - tbs["center"][2].as_f64().unwrap()).abs() < 1e-6);
    assert!((sphere_wc.radius - tbs["radius"].as_f64().unwrap()).abs() < 1e-6);
}

#[test]
fn cloud_property_mutations_and_dirty_tracking() {
    let mut coll = CloudCollection::new();
    let handle = coll.add(CumulusCloudOptions::default()).unwrap();

    // Check initial dirty state after add
    assert!(handle.borrow().is_dirty());
    // Clear dirty
    handle.borrow_mut().clear_dirty();
    assert!(!handle.borrow().is_dirty());

    // Mutate properties and observe dirty flags
    handle.borrow_mut().set_show(false);
    assert!(handle.borrow().is_property_dirty(SHOW_INDEX));
    assert!(!handle.borrow().show());

    handle.borrow_mut().set_position(Cartesian3::new(10.0, 20.0, 30.0));
    assert!(handle.borrow().is_property_dirty(POSITION_INDEX));

    handle.borrow_mut().set_scale(Cartesian2::new(40.0, 24.0));
    assert!(handle.borrow().is_property_dirty(SCALE_INDEX));

    handle.borrow_mut().set_maximum_size(Cartesian3::new(50.0, 30.0, 20.0));
    assert!(handle.borrow().is_property_dirty(MAXIMUM_SIZE_INDEX));

    handle.borrow_mut().set_slice(0.5);
    assert!(handle.borrow().is_property_dirty(SLICE_INDEX));

    handle.borrow_mut().set_brightness(0.6);
    assert!(handle.borrow().is_property_dirty(BRIGHTNESS_INDEX));

    handle.borrow_mut().set_color(Color::RED);
    assert!(handle.borrow().is_property_dirty(COLOR_INDEX));

    let dirty_counts = coll.properties_changed_count();
    for count in dirty_counts {
        assert_eq!(count, 1);
    }

    // Atomic replacement
    handle
        .borrow_mut()
        .set_options(CumulusCloudOptions {
            show: true,
            position: Cartesian3::ZERO,
            scale: Some(Cartesian2::new(12.0, 8.0)),
            maximum_size: Some(Cartesian3::new(12.0, 8.0, 6.0)),
            slice: -1.0,
            brightness: 1.0,
            color: Color::WHITE,
            cloud_type: CloudType::Cumulus,
        })
        .unwrap();

    assert_eq!(handle.borrow().scale(), Cartesian2::new(12.0, 8.0));
    assert_eq!(handle.borrow().maximum_size(), Cartesian3::new(12.0, 8.0, 6.0));
}

#[test]
fn constructor_accepts_numeric_values_as_cesium_does() {
    let mut coll = CloudCollection::new();

    // Invalid slice NaN
    let err = coll.add(CumulusCloudOptions {
        slice: f64::NAN,
        ..Default::default()
    });
    assert!(err.is_ok());

    // Invalid brightness NaN
    let err = coll.add(CumulusCloudOptions {
        brightness: f64::NAN,
        ..Default::default()
    });
    assert!(err.is_ok());

    // Non-finite position
    let err = coll.add(CumulusCloudOptions {
        position: Cartesian3::new(f64::INFINITY, 0.0, 0.0),
        ..Default::default()
    });
    assert!(err.is_ok());

    // Negative scale
    let err = coll.add(CumulusCloudOptions {
        scale: Some(Cartesian2::new(-1.0, 10.0)),
        ..Default::default()
    });
    assert!(err.is_ok());

    // Negative maximum size
    let err = coll.add(CumulusCloudOptions {
        maximum_size: Some(Cartesian3::new(10.0, -5.0, 10.0)),
        ..Default::default()
    });
    assert!(err.is_ok());
}

#[test]
fn drop_collection_detaches_contained_clouds() {
    let handle;
    {
        let mut coll = CloudCollection::new();
        handle = coll.add(CumulusCloudOptions::default()).unwrap();
        assert!(coll.contains(&handle));
        assert!(handle.borrow().is_attached());
    }
    // After collection drop, handle is detached
    assert!(!handle.borrow().is_attached());
}

#[test]
fn borrowed_cloud_does_not_corrupt_collection_on_removal_or_compaction() {
    let mut collection = CloudCollection::new();
    let first = collection.add(CumulusCloudOptions::default()).unwrap();
    let second = collection.add(CumulusCloudOptions::default()).unwrap();
    let guard = first.borrow();
    assert!(collection.contains(&first));
    assert!(!collection.remove(&first));
    assert!(collection.contains(&first));
    drop(guard);
    assert!(collection.remove(&first));
    let guard = second.borrow();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| collection.compact())).is_err());
    drop(guard);
    assert_eq!(collection.len(), 1);
    assert!(collection.contains(&second));
}

#[test]
fn cloud_removal_detaches_and_compaction_reindexes() {
    let mut collection = CloudCollection::new();
    let first = collection.add(CumulusCloudOptions::default()).unwrap();
    let second = collection.add(CumulusCloudOptions::default()).unwrap();
    assert!(collection.remove(&first));
    assert_eq!(first.borrow().collection_index(), usize::MAX);
    assert_eq!(collection.len(), 1);
    assert_eq!(second.borrow().collection_index(), 0);
}
