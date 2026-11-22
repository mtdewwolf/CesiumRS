//! WebGL2 browser validation harness for `Scene/Model/Model` and
//! `Scene/Model/Model3DTileContent` (glTF/GLB, B3DM, I3DM, PNTS, GeoJSON).
#[cfg(target_arch = "wasm32")]
mod browser {
    use cesium_rs::{
        core::{
            browser_host::{BrowserImageHost, BrowserTransport},
            *,
        },
        renderer::{browser::BrowserGl, *},
        scene::{
            gltf_pipeline::BufferSource,
            model::{
                CustomShader, CustomShaderMode, CustomShaderOptions, CustomShaderTranslucencyMode, LightingModel,
                Model, Model3DTileContent, ModelOptions, ModelTileFactory,
            },
            *,
        },
    };
    use serde_json::{json, Value};
    use std::{cell::RefCell, f64::consts::FRAC_PI_3, rc::Rc, sync::Arc};
    use wasm_bindgen::prelude::*;

    fn error(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }

    fn make_camera_frame(size: u32, frame_number: u32) -> FrameState {
        let mut camera = Camera::new(CameraOptions::new(size as f64, size as f64));
        camera.position = Cartesian3::new(0., 0., 5.);
        camera.direction = Cartesian3::new(0., 0., -1.);
        camera.up = Cartesian3::new(0., 1., 0.);
        camera.right = Cartesian3::new(1., 0., 0.);
        camera.frustum = CameraFrustum::Perspective(PerspectiveFrustum {
            fov: FRAC_PI_3,
            aspect_ratio: 1.0,
            near: 0.1,
            far: 100.0,
            x_offset: 0.0,
            y_offset: 0.0,
        });
        let snapshot = camera.uniform_camera();
        let culling = camera
            .frustum
            .compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        let mut frame = FrameState::new(
            UniformFrame::new(snapshot, JulianDate::new(2451545., 0., TimeStandard::Tai)),
            culling,
            [size, size],
        );
        frame.frame_number = frame_number;
        frame.passes.render = true;
        frame.stencil_buffer = true;
        frame
    }

    fn finish_frame(frame: &mut FrameState) -> Result<(), RuntimeError> {
        for cb in std::mem::take(&mut frame.after_render) {
            cb()?;
        }
        Ok(())
    }

    fn render_commands(
        context: &Context<BrowserGl>,
        frame: &mut FrameState,
        size: u32,
    ) -> Result<(Vec<u8>, usize), RuntimeError> {
        context.uniform_state().update(&frame.uniform, size)?;
        let viewport = Viewport { x: 0, y: 0, width: size, height: size };
        let pass_state = PassState { viewport: Some(viewport), ..Default::default() };
        ClearCommand {
            color: Some([0., 0., 0., 0.]),
            depth: Some(1.),
            stencil: Some(0),
            ..Default::default()
        }
        .execute(context, &pass_state)?;
        let count = frame.command_list.len();
        for cmd in std::mem::take(&mut frame.command_list) {
            let submission = SceneSubmission {
                pass: cmd.pass,
                frustum: 0,
                near: 0.1,
                far: 100.0,
                draw_mode: SceneDrawMode::default(),
                oit: None,
                metadata_pick: None,
                framebuffer: None,
                viewport: Some(viewport),
                scissor_test: None,
                blending_enabled: None,
                metadata: cmd.metadata.clone(),
                clipping_planes: None,
                tileset_draw: None,
            };
            (cmd.execute)(submission)?;
        }
        let pixels = context.read_pixels(TextureRegion { x: 0, y: 0, width: size, height: size })?;
        Ok((pixels, count))
    }

    fn parse_matrix(v: &Value) -> Option<Matrix4> {
        let arr = v.as_array()?;
        let mut m = [0.0; 16];
        for (i, item) in arr.iter().take(16).enumerate() {
            m[i] = item.as_f64()?;
        }
        Some(Matrix4(m))
    }

    fn parse_color(v: &Value) -> Option<Color> {
        let a = v.as_array()?;
        Some(Color::new(
            a.first()?.as_f64()?,
            a.get(1)?.as_f64()?,
            a.get(2)?.as_f64()?,
            a.get(3)?.as_f64()?,
        ))
    }

    fn parse_blend_mode(s: &str) -> ColorBlendMode {
        match s {
            "replace" => ColorBlendMode::Replace,
            "mix" => ColorBlendMode::Mix,
            _ => ColorBlendMode::Highlight,
        }
    }

    /// Loads a binary GLB with `Model::from_gltf`, applies options (color, blend
    /// mode, scale, modelMatrix, customShader, style, show toggle), renders on
    /// WebGL2, and returns pixel readbacks + statistics.
    #[wasm_bindgen]
    pub async fn render_gltf_model(gl: JsValue, glb_bytes: Vec<u8>, config_json: &str) -> Result<String, JsValue> {
        let cfg: Value = serde_json::from_str(config_json).map_err(error)?;
        let size = cfg["size"].as_u64().unwrap_or(64) as u32;
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let cache = ResourceCache::new(ResourceClient::new(Rc::new(BrowserTransport::default())));
        let host = Rc::new(BrowserImageHost);
        let resource = Resource::new("https://validation.local/model.glb").ok_or_else(|| error("invalid url"))?;

        let mut options = ModelOptions::default();
        options.id = Some("gltf-validation".into());
        options.cull = false;
        if let Some(m) = parse_matrix(&cfg["modelMatrix"]) {
            options.model_matrix = m;
        }
        if let Some(s) = cfg["scale"].as_f64() {
            options.scale = s;
        }
        if let Some(c) = parse_color(&cfg["color"]) {
            options.color = Some(c);
        }
        if let Some(mode) = cfg["colorBlendMode"].as_str() {
            options.color_blend_mode = parse_blend_mode(mode);
        }
        if let Some(amt) = cfg["colorBlendAmount"].as_f64() {
            options.color_blend_amount = amt;
        }

        let mut model = Model::from_gltf(
            cache,
            resource,
            None,
            Some(GltfJsonInput::Bytes(BufferSource::new(glb_bytes))),
            context.clone(),
            host,
            options,
        )
        .await
        .map_err(error)?;

        if let Some(fs_text) = cfg["customShaderFs"].as_str() {
            let shader = CustomShader::new(CustomShaderOptions {
                mode: CustomShaderMode::ModifyMaterial,
                lighting_model: Some(LightingModel::Unlit),
                translucency_mode: CustomShaderTranslucencyMode::Inherit,
                uniforms: Default::default(),
                varyings: Default::default(),
                vertex_shader_text: None,
                fragment_shader_text: Some(fs_text.to_owned()),
            })
            .map_err(error)?;
            model.custom_shader = Some(Rc::new(shader));
        }

        if let Some(style_json) = cfg.get("style").filter(|v| !v.is_null()) {
            let style = Cesium3DTileStyle::new(Some(style_json)).map_err(error)?;
            model.set_style(Some(Rc::new(RefCell::new(style))));
        }

        // Drive loader + build draw commands until ready
        let mut frame_num = 1u32;
        for _ in 0..8 {
            let mut frame = make_camera_frame(size, frame_num);
            frame_num += 1;
            model.update(&mut frame).map_err(error)?;
            finish_frame(&mut frame).map_err(error)?;
            if model.ready() {
                break;
            }
        }
        if !model.ready() {
            return Err(error("Model did not become ready"));
        }

        // Render frames (supports optional scripted mutations like show toggle / transform update)
        let mut frames_out = Vec::new();
        let steps = cfg["steps"].as_array().cloned().unwrap_or_else(|| vec![json!({})]);
        for step in steps {
            if let Some(show) = step["show"].as_bool() {
                model.options.show = show;
            }
            if let Some(m) = parse_matrix(&step["modelMatrix"]) {
                model.options.model_matrix = m;
            }
            let mut frame = make_camera_frame(size, frame_num);
            frame_num += 1;
            model.update(&mut frame).map_err(error)?;
            let (pixels, commands) = render_commands(&context, &mut frame, size).map_err(error)?;
            finish_frame(&mut frame).map_err(error)?;
            frames_out.push(json!({ "pixels": pixels, "commands": commands }));
        }

        let stats = json!({
            "ready": model.ready(),
            "triangles": model.statistics.triangles_length,
            "points": model.statistics.points_length,
            "geometryBytes": model.statistics.geometry_byte_length,
        });
        model.destroy().map_err(error)?;
        let destroyed = model.is_destroyed();
        Ok(json!({ "frames": frames_out, "stats": stats, "destroyed": destroyed }).to_string())
    }

    /// Loads a 3D Tiles content payload (glb, b3dm, i3dm, pnts, geojson) via
    /// `ModelTileFactory` -> `Model3DTileContent`, exercises readiness, metrics,
    /// feature queries, style & debug settings, renders on WebGL2, and destroys.
    #[wasm_bindgen]
    pub async fn render_tile_content(
        gl: JsValue,
        format: &str,
        bytes: Vec<u8>,
        config_json: &str,
    ) -> Result<String, JsValue> {
        let cfg: Value = serde_json::from_str(config_json).map_err(error)?;
        let size = cfg["size"].as_u64().unwrap_or(64) as u32;
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let cache = ResourceCache::new(ResourceClient::new(Rc::new(BrowserTransport::default())));
        let host = Rc::new(BrowserImageHost);
        let resource =
            Resource::new(&format!("https://validation.local/tile.{format}")).ok_or_else(|| error("invalid url"))?;

        let kind = match format {
            "glb" => Cesium3DTileContentType::GltfBinary,
            "b3dm" => Cesium3DTileContentType::Batched3DModel,
            "i3dm" => Cesium3DTileContentType::Instanced3DModel,
            "pnts" => Cesium3DTileContentType::PointCloud,
            "geojson" => Cesium3DTileContentType::GeoJson,
            other => return Err(error(format!("unsupported format {other}"))),
        };

        let mut options = ModelOptions::default();
        options.cull = false;
        if let Some(m) = parse_matrix(&cfg["modelMatrix"]) {
            options.model_matrix = m;
        }

        // For b3dm we also directly inspect Model3DTileContent feature APIs;
        // for all formats we also verify ModelTileFactory::create works.
        let mut content = if format == "b3dm" {
            let model = Model::from_b3dm(
                cache,
                resource.clone(),
                None,
                Arc::new(bytes),
                0,
                context.clone(),
                host,
                options,
            )
            .await
            .map_err(error)?;
            Model3DTileContent::new(model, 0, resource)
        } else {
            let factory = ModelTileFactory {
                context: context.clone(),
                cache,
                host,
                options,
            };
            let request = TileContentRequest {
                tile: 0,
                resource: resource.clone(),
                base_resource: resource.clone(),
                implicit: None,
                cancelled: Rc::new(std::cell::Cell::new(false)),
                header: json!({}),
            };
            let mut boxed = factory.create(request, kind, bytes).await.map_err(error)?;
            // Drive through the TilesetContent trait interface!
            let mut frame_num = 1u32;
            for _ in 0..8 {
                let mut frame = make_camera_frame(size, frame_num);
                frame_num += 1;
                let ready = boxed.process_with_frame(&mut frame).map_err(error)?;
                finish_frame(&mut frame).map_err(error)?;
                if ready {
                    break;
                }
            }
            let metrics = boxed.metrics();
            let mut frame = make_camera_frame(size, frame_num);
            boxed.update(&mut frame).map_err(error)?;
            let passes: Vec<u32> = frame.command_list.iter().map(|c| c.pass as u32).collect();
            let (pixels, commands) = render_commands(&context, &mut frame, size).map_err(error)?;
            finish_frame(&mut frame).map_err(error)?;
            boxed.destroy().map_err(error)?;
            return Ok(json!({
                "ready": true,
                "metrics": {
                    "features": metrics.features,
                    "points": metrics.points,
                    "triangles": metrics.triangles,
                    "geometryBytes": metrics.geometry_bytes,
                    "textureBytes": metrics.texture_bytes,
                    "batchTableBytes": metrics.batch_table_bytes,
                },
                "passes": passes,
                "commands": commands,
                "pixels": pixels,
                "destroyed": true,
            })
            .to_string());
        };

        // Drive B3DM Model3DTileContent through process_with_frame until ready
        let mut frame_num = 1u32;
        for _ in 0..8 {
            let mut frame = make_camera_frame(size, frame_num);
            frame_num += 1;
            let ready = content.process_with_frame(&mut frame).map_err(error)?;
            finish_frame(&mut frame).map_err(error)?;
            if ready {
                break;
            }
        }
        if !content.ready() {
            return Err(error("Model3DTileContent did not become ready"));
        }

        let metrics = content.metrics();
        let features_length = content.features_length();
        let has_kind = content.has_property(0, "kind").map_err(error)?;
        let has_missing = content.has_property(0, "nonexistent").map_err(error)?;
        let feat0 = content.get_feature(0).map_err(error)?;
        let feat1 = content.get_feature(1).map_err(error)?;
        let feat0_kind = feat0.get_property("kind").map_err(error)?;
        let feat1_kind = feat1.get_property("kind").map_err(error)?;

        // Frame 1: Base unstyled render
        let mut frame = make_camera_frame(size, frame_num);
        frame_num += 1;
        content.update(&mut frame).map_err(error)?;
        let passes: Vec<u32> = frame.command_list.iter().map(|c| c.pass as u32).collect();
        let (base_pixels, base_cmds) = render_commands(&context, &mut frame, size).map_err(error)?;
        finish_frame(&mut frame).map_err(error)?;

        // Frame 2: Styled render via Cesium3DTileStyle
        let mut styled_pixels = None;
        if let Some(style_json) = cfg.get("style").filter(|v| !v.is_null()) {
            let style = Cesium3DTileStyle::new(Some(style_json)).map_err(error)?;
            content.model.set_style(Some(Rc::new(RefCell::new(style))));
            let mut frame = make_camera_frame(size, frame_num);
            frame_num += 1;
            content.update(&mut frame).map_err(error)?;
            let (px, _) = render_commands(&context, &mut frame, size).map_err(error)?;
            finish_frame(&mut frame).map_err(error)?;
            styled_pixels = Some(px);
        }

        // Frame 3: Clear style, then apply debug color override via Model3DTileContent::apply_debug_settings
        content.model.set_style(None);
        let mut clear_style_frame = make_camera_frame(size, frame_num);
        frame_num += 1;
        content.update(&mut clear_style_frame).map_err(error)?;
        finish_frame(&mut clear_style_frame).map_err(error)?;
        content.apply_debug_settings(true, Color::LIME).map_err(error)?;
        let mut frame = make_camera_frame(size, frame_num);
        content.update(&mut frame).map_err(error)?;
        let (debug_pixels, _) = render_commands(&context, &mut frame, size).map_err(error)?;
        finish_frame(&mut frame).map_err(error)?;

        content.destroy().map_err(error)?;
        let destroyed = content.is_destroyed();

        Ok(json!({
            "ready": true,
            "metrics": {
                "features": metrics.features,
                "points": metrics.points,
                "triangles": metrics.triangles,
                "geometryBytes": metrics.geometry_bytes,
                "textureBytes": metrics.texture_bytes,
                "batchTableBytes": metrics.batch_table_bytes,
            },
            "featuresLength": features_length,
            "hasKind": has_kind,
            "hasMissing": has_missing,
            "feat0Kind": format!("{feat0_kind:?}"),
            "feat1Kind": format!("{feat1_kind:?}"),
            "passes": passes,
            "commands": base_cmds,
            "basePixels": base_pixels,
            "styledPixels": styled_pixels,
            "debugPixels": debug_pixels,
            "destroyed": destroyed,
        })
        .to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {}
