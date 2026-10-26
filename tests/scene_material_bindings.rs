//! Renderer-facing material updates and complete appearance overrides.
use cesium_rs::{core::*, renderer::*, scene::*};
use std::sync::{Arc, Mutex};

#[test]
fn complete_render_state_preserves_false_values_and_all_other_fields() {
    let mut state = RenderState { depth_mask: false, ..Default::default() };
    state.depth_test.enabled = false;
    state.cull.enabled = false;
    state.blending.enabled = false;
    state.stencil_test.enabled = true;
    state.stencil_test.reference = 7;
    state.stencil_mask = 0x3f;
    state.color_mask.red = false;
    state.polygon_offset.enabled = true;
    state.polygon_offset.factor = 3.;
    state.polygon_offset.units = -2.;
    state.viewport = Some(Viewport { x: 2, y: 3, width: 32, height: 64 });
    state.scissor_test.enabled = true;
    state.scissor_test.rectangle = Viewport { x: 4, y: 5, width: 6, height: 7 };
    state.sample_coverage.enabled = true;
    state.sample_coverage.value = 0.5;
    state.sample_coverage.invert = true;
    state.line_width = 2.5;
    state.depth_range.near = 0.2;
    state.depth_range.far = 0.8;

    for translucent in [false, true] {
        for closed in [false, true] {
            assert_eq!(default_render_state(translucent, closed, Some(state.clone())), state);
            let appearance = PerInstanceColorAppearance::new(PerInstanceColorAppearanceOptions {
                translucent: Some(translucent),
                closed: Some(closed),
                render_state: Some(state.clone()),
                ..Default::default()
            });
            assert_eq!(appearance.render_state(), &state);
        }
    }
}

#[test]
fn notified_polygon_handles_can_be_removed_without_changing_clone_semantics() {
    let mut collection = ClippingPolygonCollection::default();
    let events = Arc::new(Mutex::new(Vec::<ClippingPolygonEvent>::new()));
    let recorded = Arc::clone(&events);
    collection.polygon_added.add_event_listener(move |event| recorded.lock().unwrap().push(event.clone()));
    let handle = collection.add(ClippingPolygon::new(ClippingPolygonOptions {
        positions: vec![Cartesian3::UNIT_X, Cartesian3::UNIT_Y, Cartesian3::UNIT_Z],
        ellipsoid: None,
    }));
    let event = events.lock().unwrap().pop().unwrap();
    assert_eq!(event.index, 0);
    assert_eq!(handle.id, event.polygon.id);
    assert!(collection.contains(&event.polygon));
    assert!(!collection.contains(&handle.clone()));
    assert!(collection.remove(&event.polygon));
    assert!(collection.is_empty());
}

#[test]
fn rejected_uniform_type_change_leaves_all_generated_bindings_unchanged() {
    let mut material = Material::new(MaterialOptions {
        fabric: Fabric::default()
            .uniform("color_scale", MaterialValue::Float(2.))
            .uniform("color", MaterialValue::from_color(Color::RED))
            .component("diffuse", "color.rgb * color_scale"),
        ..Default::default()
    });
    let before = material.uniform_bindings();
    assert!(material.set_uniform("color", MaterialValue::Float(3.)).is_err());
    assert_eq!(material.uniform_bindings(), before);
    material.set_uniform("color", MaterialValue::from_color(Color::GREEN)).unwrap();
    let bindings = material.uniform_bindings();
    assert_eq!(bindings[0], ("color_scale_0".into(), MaterialValue::Float(2.)));
    assert_eq!(bindings[1], ("color_1".into(), MaterialValue::from_color(Color::GREEN)));
}

#[test]
fn nested_texture_resolution_updates_parent_bindings_and_clones_remain_independent() {
    let image = Fabric::default()
        .uniform("image", MaterialValue::Sampler2D(MaterialImage::Url("https://textures.test/image.png".into())))
        .component("diffuse", "texture(image, materialInput.st).rgb");
    let middle = Fabric::default().sub_material("leaf", image).component("diffuse", "leaf.diffuse");
    let mut material = Material::new(MaterialOptions {
        fabric: Fabric::default().sub_material("middle", middle).component("diffuse", "middle.diffuse"),
        ..Default::default()
    });
    assert!(material.update_textures().is_err());
    let leaf = &mut material.materials[0].1.materials[0].1;
    assert_eq!(leaf.unresolved_textures().len(), 1);
    leaf.set_texture("image", "loaded-texture");
    material.update_textures().unwrap();
    assert_eq!(
        material.uniform_bindings(),
        vec![("image_0".into(), MaterialValue::Sampler2D(MaterialImage::Texture("loaded-texture".into())))]
    );

    let mut copy = material.clone();
    copy.materials[0].1.materials[0].1.set_texture("image", "copy-texture");
    assert_ne!(copy.uniform_bindings(), material.uniform_bindings());
    assert_eq!(
        material.uniform_bindings()[0].1,
        MaterialValue::Sampler2D(MaterialImage::Texture("loaded-texture".into()))
    );
}
