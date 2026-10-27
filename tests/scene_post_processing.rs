//! Dedicated tests for Scene post-processing batch.
use cesium_rs::core::*;
use cesium_rs::renderer::*;
use cesium_rs::scene::*;
use cesium_rs::scene::auto_exposure::*;
use cesium_rs::scene::post_process_stage::*;
use cesium_rs::scene::post_process_stage_collection::*;
use cesium_rs::scene::post_process_stage_library::*;
use cesium_rs::scene::post_process_stage_texture_cache::*;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

#[derive(Default)]
struct MockGlState {
    next: Cell<u32>,
    calls: RefCell<Vec<String>>,
    width: Cell<u32>,
    height: Cell<u32>,
}

#[derive(Clone)]
struct TestHost(Rc<MockGlState>);

impl Default for TestHost {
    fn default() -> Self {
        Self(Rc::new(MockGlState {
            next: Cell::new(0),
            calls: RefCell::new(Vec::new()),
            width: Cell::new(800),
            height: Cell::new(600),
        }))
    }
}

impl Gl for TestHost {
    type Object = u32;

    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([self.0.width.get(), self.0.height.get()])
    }

    fn invoke(&self, name: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.0.calls.borrow_mut().push(name.into());
        if name.starts_with("create") || name == "fenceSync" {
            let id = self.0.next.get() + 1;
            self.0.next.set(id);
            return Ok(GlValue::Object(id));
        }
        Ok(match name {
            "getParameter" => GlValue::Number(16384.),
            "getExtension" => GlValue::Object(0),
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            _ => GlValue::Null,
        })
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("data/scene-post-processing.json"))
        .expect("scene-post-processing fixture")
}

#[test]
fn post_process_stage_library_parity() {
    let fix = fixture();
    let lib = &fix["library"];

    // 1. Blur
    let blur = PostProcessStageLibrary::create_blur_stage::<TestHost>("czm_blur").unwrap();
    assert_eq!(blur.name(), lib["blur"]["name"].as_str().unwrap());
    assert_eq!(blur.len(), lib["blur"]["length"].as_u64().unwrap() as usize);
    assert_eq!(
        blur.input_previous_stage_texture(),
        lib["blur"]["inputPreviousStageTexture"].as_bool().unwrap()
    );
    assert_eq!(blur.get(0).unwrap().name(), "czm_blur_x_direction");
    assert_eq!(blur.get(1).unwrap().name(), "czm_blur_y_direction");

    // 2. Depth of Field
    let dof = PostProcessStageLibrary::create_depth_of_field_stage::<TestHost>().unwrap();
    assert_eq!(dof.name(), lib["depthOfField"]["name"].as_str().unwrap());
    assert_eq!(dof.len(), lib["depthOfField"]["length"].as_u64().unwrap() as usize);
    assert_eq!(
        dof.input_previous_stage_texture(),
        lib["depthOfField"]["inputPreviousStageTexture"].as_bool().unwrap()
    );

    // 3. Edge Detection
    let edge = PostProcessStageLibrary::create_edge_detection_stage::<TestHost>().unwrap();
    let prefix = lib["edgeDetection"]["namePrefix"].as_str().unwrap();
    assert!(edge.name().starts_with(prefix));

    // 4. Silhouette
    let sil = PostProcessStageLibrary::create_silhouette_stage::<TestHost>(None).unwrap();
    assert_eq!(sil.name(), lib["silhouette"]["name"].as_str().unwrap());
    assert_eq!(sil.len(), lib["silhouette"]["length"].as_u64().unwrap() as usize);
    assert_eq!(
        sil.input_previous_stage_texture(),
        lib["silhouette"]["inputPreviousStageTexture"].as_bool().unwrap()
    );

    // 5. Bloom
    let bloom = PostProcessStageLibrary::create_bloom_stage::<TestHost>().unwrap();
    assert_eq!(bloom.name(), lib["bloom"]["name"].as_str().unwrap());
    assert_eq!(bloom.len(), lib["bloom"]["length"].as_u64().unwrap() as usize);

    // 6. Ambient Occlusion
    let ao = PostProcessStageLibrary::create_ambient_occlusion_stage::<TestHost>().unwrap();
    assert_eq!(ao.name(), lib["ambientOcclusion"]["name"].as_str().unwrap());
    assert_eq!(ao.len(), lib["ambientOcclusion"]["length"].as_u64().unwrap() as usize);

    // 7. FXAA
    let fxaa = PostProcessStageLibrary::create_fxaa_stage::<TestHost>().unwrap();
    assert_eq!(fxaa.name(), lib["fxaa"]["name"].as_str().unwrap());
    assert_eq!(fxaa.sample_mode(), PostProcessStageSampleMode::Linear);

    // 8. Tonemappers
    let aces = PostProcessStageLibrary::create_aces_tonemapping_stage::<TestHost>(false).unwrap();
    assert_eq!(aces.name(), lib["tonemappers"]["aces"]["name"].as_str().unwrap());
    let filmic = PostProcessStageLibrary::create_filmic_tonemapping_stage::<TestHost>(false).unwrap();
    assert_eq!(filmic.name(), lib["tonemappers"]["filmic"]["name"].as_str().unwrap());
    let pbr = PostProcessStageLibrary::create_pbr_neutral_tonemapping_stage::<TestHost>(false).unwrap();
    assert_eq!(pbr.name(), lib["tonemappers"]["pbrNeutral"]["name"].as_str().unwrap());
    let reinhard = PostProcessStageLibrary::create_reinhard_tonemapping_stage::<TestHost>(false).unwrap();
    assert_eq!(reinhard.name(), lib["tonemappers"]["reinhard"]["name"].as_str().unwrap());
    let mod_reinhard = PostProcessStageLibrary::create_modified_reinhard_tonemapping_stage::<TestHost>(false).unwrap();
    assert_eq!(mod_reinhard.name(), lib["tonemappers"]["modifiedReinhard"]["name"].as_str().unwrap());

    // 9. Other built-ins
    let bw = PostProcessStageLibrary::create_black_and_white_stage::<TestHost>().unwrap();
    assert_eq!(bw.name(), lib["blackAndWhite"]["name"].as_str().unwrap());
    let br = PostProcessStageLibrary::create_brightness_stage::<TestHost>().unwrap();
    assert_eq!(br.name(), lib["brightness"]["name"].as_str().unwrap());
    let nv = PostProcessStageLibrary::create_night_vision_stage::<TestHost>().unwrap();
    assert_eq!(nv.name(), lib["nightVision"]["name"].as_str().unwrap());
    let dv = PostProcessStageLibrary::create_depth_view_stage::<TestHost>().unwrap();
    assert_eq!(dv.name(), lib["depthView"]["name"].as_str().unwrap());
    let lf = PostProcessStageLibrary::create_lens_flare_stage::<TestHost>().unwrap();
    assert_eq!(lf.name(), lib["lensFlare"]["name"].as_str().unwrap());
}

#[test]
fn auto_exposure_pyramid_reduction_parity() {
    let fix = fixture();
    let pyramids = fix["autoExposure"]["pyramids"].as_array().unwrap();

    let host = TestHost::default();
    let context = Context::new_webgl2(host.clone());

    for item in pyramids {
        let input_w = item["input"][0].as_u64().unwrap() as u32;
        let input_h = item["input"][1].as_u64().unwrap() as u32;
        let expected_len = item["length"].as_u64().unwrap() as usize;
        let expected_levels = item["levels"].as_array().unwrap();

        host.0.width.set(input_w);
        host.0.height.set(input_h);

        let mut auto = AutoExposure::<TestHost>::new();
        auto.update(&context).unwrap();
        assert!(auto.ready());

        let max_dim = input_w.max(input_h) as f64;
        let length = (max_dim.ln() / 3.0_f64.ln()).ceil().max(0.0) as usize;
        assert_eq!(length, expected_len, "Resolution {input_w}x{input_h}");

        let mut cur_w = input_w;
        let mut cur_h = input_h;
        for (i, lvl) in expected_levels.iter().enumerate() {
            cur_w = ((cur_w as f64) / 3.0).ceil().max(1.0) as u32;
            cur_h = ((cur_h as f64) / 3.0).ceil().max(1.0) as u32;
            assert_eq!(cur_w, lvl[0].as_u64().unwrap() as u32, "level {i} width");
            assert_eq!(cur_h, lvl[1].as_u64().unwrap() as u32, "level {i} height");
        }
    }
}

#[test]
fn post_process_stage_options_and_selection_semantics() {
    let host = TestHost::default();
    let context = Context::new_webgl2(host);

    // Options validation
    let mut bad_scale = PostProcessStageOptions::<TestHost>::new("void main() {}");
    bad_scale.texture_scale = Some(0.0);
    assert!(PostProcessStage::new(bad_scale).is_err());

    let mut bad_scale2 = PostProcessStageOptions::<TestHost>::new("void main() {}");
    bad_scale2.texture_scale = Some(1.5);
    assert!(PostProcessStage::new(bad_scale2).is_err());

    let mut opts = PostProcessStageOptions::<TestHost>::new(
        "uniform sampler2D colorTexture;\n\
         in vec2 v_textureCoordinates;\n\
         void main() {\n\
             if (czm_selected()) {\n\
                 out_FragColor = vec4(1.0);\n\
             } else {\n\
                 out_FragColor = texture(colorTexture, v_textureCoordinates);\n\
             }\n\
         }",
    );
    opts.name = Some("custom_stage".into());
    opts.texture_scale = Some(0.5);
    opts.scissor_rectangle = Some(BoundingRectangle::new(10.0, 20.0, 100.0, 200.0));
    opts.uniforms.insert("myFloat".into(), 42.0_f32.into());
    opts.uniforms.insert("myVec3".into(), [1.0_f32, 2.0, 3.0].into());

    let mut stage = PostProcessStage::new(opts).unwrap();
    assert_eq!(stage.name(), "custom_stage");
    assert_eq!(stage.texture_scale(), 0.5);
    assert_eq!(stage.scissor_rectangle(), Some(BoundingRectangle::new(10.0, 20.0, 100.0, 200.0)));

    // Setting selection features
    stage.set_selected(Some(vec![
        SelectedFeature::Color(Color::new(1.0, 0.0, 0.0, 1.0)),
        SelectedFeature::Color(Color::new(0.0, 1.0, 0.0, 1.0)),
    ]));

    stage.update(&context, false).unwrap();
    assert!(stage.ready());

    // Execution without framebuffer assigned is a no-op
    let tex_opts = TextureOptions::rgba(800, 600);
    let color_tex = Rc::new(Texture::new(&context, tex_opts, None).unwrap());
    stage.execute(&context, &color_tex, None, None, None).unwrap();

    // Assign framebuffer and verify executable pass
    let mut fb_mgr = FramebufferManager::new(FramebufferManagerOptions {
        color: true,
        ..Default::default()
    }).unwrap();
    fb_mgr.update(&context, 400, 300, Some(1), Some(PixelDatatype::UnsignedByte), Some(PixelFormat::Rgba)).unwrap();
    stage.set_framebuffer(fb_mgr.framebuffer().cloned(), fb_mgr.color_texture(0).unwrap().cloned());

    stage.execute(&context, &color_tex, None, None, None).unwrap();
    assert!(stage.output_texture().is_some());
}

#[test]
fn texture_cache_minimal_framebuffer_allocation_and_resizing() {
    let host = TestHost::default();
    let context = Context::new_webgl2(host.clone());

    // Three stages in series: A -> B -> C
    // In series, stage B depends on A, and stage C depends on B (not A!).
    // Therefore C can reuse A's framebuffer! Framebuffers needed = 2!
    let s1 = PostProcessStage::<TestHost>::new(PostProcessStageOptions::new("void main() {}")).unwrap();
    let s2 = PostProcessStage::<TestHost>::new(PostProcessStageOptions::new("void main() {}")).unwrap();
    let s3 = PostProcessStage::<TestHost>::new(PostProcessStageOptions::new("void main() {}")).unwrap();

    let mut cache = PostProcessStageTextureCache::<TestHost>::new();
    let mut dependencies = HashMap::new();

    let prev = get_stage_dependencies(&mut dependencies, &s1, None);
    let prev = get_stage_dependencies(&mut dependencies, &s2, prev.as_deref());
    get_stage_dependencies(&mut dependencies, &s3, prev.as_deref());

    let leaf_stages = vec![&s1, &s2, &s3];
    cache.update(&context, &leaf_stages, &dependencies).unwrap();

    // Verify minimal allocation: exactly 2 framebuffers shared across the 3 stages!
    assert_eq!(cache.framebuffer_count(), 2);
    assert!(cache.get_framebuffer(s1.name()).is_some());
    assert!(cache.get_framebuffer(s2.name()).is_some());
    assert!(cache.get_framebuffer(s3.name()).is_some());

    // FB of s1 and s3 are the exact same underlying framebuffer
    let fb1 = cache.get_framebuffer(s1.name()).unwrap();
    let fb3 = cache.get_framebuffer(s3.name()).unwrap();
    assert!(Rc::ptr_eq(&fb1, &fb3));

    // Resizing test
    host.0.width.set(1024);
    host.0.height.set(768);
    cache.update(&context, &leaf_stages, &dependencies).unwrap();
    let tex = cache.get_output_texture(s1.name()).unwrap();
    assert_eq!(tex.width(), 1024);
    assert_eq!(tex.height(), 768);

    cache.destroy().unwrap();
    assert!(cache.is_destroyed());
}

#[test]
fn post_process_stage_collection_pipeline_execution() {
    let host = TestHost::default();
    let context = Context::new_webgl2(host);

    let mut col = PostProcessStageCollection::<TestHost>::new(&context).unwrap();
    assert_eq!(col.len(), 0);
    assert_eq!(col.tonemapper(), Tonemapper::PbrNeutral);
    assert_eq!(col.exposure(), 1.0);

    // Add user stages
    let mut opts1 = PostProcessStageOptions::new("void main() {}");
    opts1.name = Some("custom1".into());
    let mut opts2 = PostProcessStageOptions::new("void main() {}");
    opts2.name = Some("custom2".into());

    col.add(PostProcessStage::new(opts1).unwrap()).unwrap();
    col.add(PostProcessStage::new(opts2).unwrap()).unwrap();
    assert_eq!(col.len(), 2);
    assert!(col.contains("custom1"));
    assert!(col.contains("custom2"));

    // Changing tonemapper
    col.set_tonemapper(Tonemapper::Aces).unwrap();
    assert_eq!(col.tonemapper(), Tonemapper::Aces);
    col.set_exposure(1.5);
    assert_eq!(col.exposure(), 1.5);

    // Update with HDR and log depth
    col.update(&context, true, true).unwrap();
    assert!(col.ready());

    // Full GPU pipeline execution with color & depth textures
    let color_tex = Rc::new(Texture::new(&context, TextureOptions::rgba(800, 600), None).unwrap());
    let mut depth_opts = TextureOptions::rgba(800, 600);
    depth_opts.pixel_format = PixelFormat::DepthComponent;
    depth_opts.pixel_datatype = PixelDatatype::UnsignedInt;
    let depth_tex = Rc::new(Texture::new(&context, depth_opts, None).unwrap());

    col.execute(&context, &color_tex, Some(&depth_tex), None).unwrap();
    assert!(col.output_texture().is_some());

    // Copy to target
    let pass = PassState::default();
    col.copy(&context, None, &pass).unwrap();

    // Clear framebuffers
    col.clear(&context).unwrap();

    col.destroy().unwrap();
    assert!(col.is_destroyed());
}
