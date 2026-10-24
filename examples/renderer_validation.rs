//! Separate WASM artifact used by the renderer parity browser harness.
#[cfg(target_arch = "wasm32")]
mod browser {
    use cesium_rs::{
        core::*,
        renderer::{browser::BrowserGl, *},
    };
    use serde_json::json;
    use wasm_bindgen::prelude::*;
    fn error(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }
    #[wasm_bindgen]
    pub async fn scene_image_texture(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use cesium_rs::{
            core::{
                browser_host::{BrowserImageHost, BrowserTransport},
                ktx2_transcoder::SupportedTextureFormats,
            },
            scene::{gltf_pipeline::BufferSource, *},
        };
        use std::{collections::BTreeMap, rc::Rc, sync::Arc};
        let c: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let context = if c["webgl1"] == true {
            Context::new_webgl1(BrowserGl::new_webgl1(gl).map_err(error)?)
        } else {
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?)
        };
        let cache = ResourceCache::new(ResourceClient::new(Rc::new(BrowserTransport::default())));
        let raw = c["raw"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect::<Vec<_>>();
        let sources = BTreeMap::from([(0, BufferSource::new(raw))]);
        let resource = Resource::new(c["resource"].as_str().unwrap()).unwrap();
        let request = GltfResource {
            document: Arc::new(LoadedGltfJson {
                json: c["gltf"].clone(),
                embedded_buffers: sources.clone(),
                buffer_sources: sources,
            }),
            base: resource.clone(),
            resource,
        };
        let seed = cache
            .embedded_buffer_loader(&request.resource, 0, request.document.buffer_sources.get(&0).cloned())
            .map_err(error)?;
        let host = Rc::new(BrowserImageHost);
        let options = GltfTextureOptions {
            asynchronous: c["asynchronous"] == true,
            supported_texture_formats: SupportedTextureFormats { s3tc: c["s3tc"] == true, ..Default::default() },
            ..Default::default()
        };
        let loader = cache
            .texture_loader(&request, json!({"index":0}), context.clone(), host.clone(), options)
            .map_err(error)?;
        let duplicate =
            cache.texture_loader(&request, json!({"index":0}), context.clone(), host, options).map_err(error)?;
        let shared = loader.ptr_eq(&duplicate);
        loader.load().await.map_err(error)?;
        let loaded = cache.len();
        let mut camera = Camera::new(CameraOptions::new(8., 8.));
        let snapshot = camera.uniform_camera();
        let culling = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        let mut frame = FrameState::new(
            UniformFrame::new(snapshot, JulianDate::new(2451545., 0., TimeStandard::Tai)),
            culling,
            [8, 8],
        );
        let quota = Rc::new(ResourceUploadBudget::new(0, 0));
        frame.job_scheduler = Some(FrameResource::new(quota.clone()));
        let first = loader.process(&frame).map_err(error)?;
        quota.reset(0, 1);
        let ready = loader.process(&frame).map_err(error)?;
        let texture = loader.texture().unwrap();
        let sampler = texture.sampler();
        let bytes = texture.size_in_bytes();
        let statistics = cache.statistics().borrow().textures_byte_length;
        let processed = cache.len();
        let quad = ViewportQuad::new(&context).map_err(error)?;
        let program=ShaderProgram::from_sources(&context,&ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new("in vec2 v_textureCoordinates;uniform sampler2D image;uniform float scale;void main(){out_FragColor=texture(image,v_textureCoordinates*scale);}"))).map_err(error)?;
        let mut command = DrawCommand::new(quad.vertex_array(), &program);
        command.textures = vec![("image".into(), &*texture)];
        command.uniforms.push(("scale".into(), UniformValue::Float(vec![c["scale"].as_f64().unwrap_or(2.) as f32])));
        ClearCommand::all().execute(&context, &PassState::default()).map_err(error)?;
        command.execute(&context, &PassState::default()).map_err(error)?;
        let pixels = context.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        drop(loader);
        let alive = !texture.is_destroyed();
        drop(duplicate);
        drop(seed);
        let result = json!({"shared":shared,"loadedEntries":loaded,"processedEntries":processed,"first":first,"ready":ready,"dimensions":texture.dimensions(),"format":texture.pixel_format()as u32,"flipY":texture.flip_y(),"preMultiplyAlpha":texture.pre_multiply_alpha(),"sampler":[sampler.wrap_s as u32,sampler.wrap_t as u32,sampler.minification_filter as u32,sampler.magnification_filter as u32],"bytes":bytes,"statistics":statistics,"pixels":pixels,"aliveWithLease":alive,"destroyed":texture.is_destroyed(),"remaining":cache.len(),"finalStatistics":cache.statistics().borrow().textures_byte_length});
        drop(command);
        quad.destroy().map_err(error)?;
        program.destroy().map_err(error)?;
        context.destroy().map_err(error)?;
        Ok(result.to_string())
    }
    #[wasm_bindgen]
    pub async fn scene_geometry_buffers(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use cesium_rs::scene::{gltf_pipeline::BufferSource, *};
        use std::{collections::BTreeMap, rc::Rc, sync::Arc};
        struct NoTransport;
        impl ResourceTransport for NoTransport {
            fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
                panic!("unexpected fetch")
            }
        }
        let config: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let webgl1 = config["webgl1"] == true;
        let context = if webgl1 {
            Context::new_webgl1(BrowserGl::new_webgl1(gl).map_err(error)?)
        } else {
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?)
        };
        let cache = ResourceCache::new(ResourceClient::new(Rc::new(NoTransport)));
        let raw = config["raw"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
        let sources = BTreeMap::from([(0, BufferSource::new(raw))]);
        let resource = Resource::new("https://geometry.test/triangle.gltf").unwrap();
        let request = GltfResource {
            document: Arc::new(LoadedGltfJson {
                json: config["gltf"].clone(),
                embedded_buffers: sources.clone(),
                buffer_sources: sources,
            }),
            base: resource.clone(),
            resource,
        };
        let seed = cache
            .embedded_buffer_loader(&request.resource, 0, request.document.embedded_buffers.get(&0).cloned())
            .map_err(error)?;
        let mut camera = Camera::new(CameraOptions::new(8., 8.));
        let snapshot = camera.uniform_camera();
        let culling = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        let mut frame = FrameState::new(
            UniformFrame::new(snapshot, JulianDate::new(2451545., 0., TimeStandard::Tai)),
            culling,
            [8, 8],
        );
        let budget = Rc::new(BufferUploadBudget::new(0));
        frame.job_scheduler = Some(FrameResource::new(budget.clone()));
        let options = GpuBufferOptions {
            asynchronous: config["asynchronous"] == true,
            load_buffer: true,
            load_typed_array: config["retain"] == true,
        };
        let vertex = cache
            .vertex_buffer_loader(&request, VertexBufferSource::BufferView(0), Some(context.clone()), options)
            .map_err(error)?;
        let duplicate = cache
            .vertex_buffer_loader(&request, VertexBufferSource::BufferView(0), Some(context.clone()), options)
            .map_err(error)?;
        let shared = vertex.ptr_eq(&duplicate);
        let index = cache.index_buffer_loader(&request, 0, None, Some(context.clone()), options).map_err(error)?;
        vertex.load().await.map_err(error)?;
        index.load().await.map_err(error)?;
        let deferred = vec![vertex.process(&frame).map_err(error)?, index.process(&frame).map_err(error)?];
        budget.reset(2);
        vertex.process(&frame).map_err(error)?;
        index.process(&frame).map_err(error)?;
        let vb = vertex.buffer().unwrap();
        let ib = index.buffer().unwrap();
        let readback = if webgl1 {
            None
        } else {
            Some(vec![
                vb.get_buffer_data(0, vb.size_in_bytes()).map_err(error)?,
                ib.get_buffer_data(0, ib.size_in_bytes()).map_err(error)?,
            ])
        };
        let va = VertexArray::new(&context, vec![VertexAttribute::buffer(0, vb.clone(), 2)], Some(ib.clone()))
            .map_err(error)?;
        let program = ShaderProgram::from_sources(
            &context,
            &ShaderProgramOptions {
                vertex_shader_source: ShaderSource::new(
                    "in vec2 position;void main(){gl_Position=vec4(position,0.,1.);}",
                ),
                fragment_shader_source: ShaderSource::new("void main(){out_FragColor=vec4(0.25,0.5,0.75,1.);}"),
                attribute_locations: BTreeMap::from([("position".into(), 0)]),
            },
        )
        .map_err(error)?;
        ClearCommand::all().execute(&context, &PassState::default()).map_err(error)?;
        DrawCommand::new(&va, &program).execute(&context, &PassState::default()).map_err(error)?;
        let pixels = context.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        let retained = vec![vertex.typed_array().is_some(), index.typed_array().is_some()];
        let size = cache.statistics().borrow().geometry_byte_length;
        drop(vertex);
        let alive = !vb.is_destroyed();
        drop(duplicate);
        drop(index);
        drop(seed);
        let result = json!({"deferred":deferred,"shared":shared,"readback":readback,"pixels":pixels,"retained":retained,"geometry":size,"aliveWithLease":alive,"destroyed":[vb.is_destroyed(),ib.is_destroyed()],"remaining":cache.len(),"finalGeometry":cache.statistics().borrow().geometry_byte_length});
        va.destroy().map_err(error)?;
        program.destroy().map_err(error)?;
        context.destroy().map_err(error)?;
        Ok(result.to_string())
    }
    #[wasm_bindgen]
    pub fn scene_oit(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use cesium_rs::scene::*;
        use std::rc::Rc;
        struct Commands(Vec<SceneCommand>);
        impl ScenePrimitive for Commands {
            fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
                frame.command_list.extend(self.0.clone());
                Ok(())
            }
            fn destroy(&mut self) -> Result<(), RuntimeError> {
                Ok(())
            }
        }
        let config: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let quad = ViewportQuad::new(&context).map_err(error)?;
        let shader=Rc::new(ShaderProgram::from_sources(&context,&ShaderProgramOptions{vertex_shader_source:ShaderSource::new("in vec4 position;\nuniform float distance;\nuniform float scale;\nvoid main(){gl_Position=czm_projection*vec4(position.xy*scale*distance*0.5,-distance,1.0);}"),fragment_shader_source:ShaderSource::new("uniform vec4 color;\nvoid main(){out_FragColor=color;}"),attribute_locations:std::collections::BTreeMap::from([("position".into(),0)])}).map_err(error)?);
        let mut commands = Vec::new();
        let classification = config["classify"] == true;
        let order = if classification {
            vec![0]
        } else if config["reverse"] == true {
            vec![0, 3, 2, 1]
        } else {
            vec![0, 1, 2, 3]
        };
        for i in order {
            let mut options = SceneDrawOptions::new(quad.vertex_array().clone(), shader.clone());
            options.state.depth_test.enabled = true;
            if i != 0 {
                options.state.blending = blending_state::alpha_blend();
                options.state.depth_mask = false;
            }
            options.uniforms = vec![
                (
                    "distance".into(),
                    UniformValue::Float(vec![
                        [if config["occluded"] == true { 925. } else { 1100. }, 1000., 950., 900.][i],
                    ]),
                ),
                ("scale".into(), UniformValue::Float(vec![if i == 0 { 0.65 } else { 1. }])),
                (
                    "color".into(),
                    UniformValue::Float(
                        [vec![0.2, 0.3, 0.4, 1.], vec![1., 0., 0., 0.4], vec![0., 1., 0., 0.5], vec![0., 0., 1., 0.2]]
                            [i]
                            .clone(),
                    ),
                ),
            ];
            commands.push(SceneCommand::from_draw(
                i.to_string(),
                context.clone(),
                if classification {
                    if config["tile"] == true {
                        Pass::Cesium3DTile
                    } else {
                        Pass::Globe
                    }
                } else if i == 0 {
                    Pass::Opaque
                } else {
                    Pass::Translucent
                },
                DrawMetadata {
                    bounding_volume: Some(BoundingSphere::new(Cartesian3::new(6378137., 0., 0.), 300.)),
                    execute_in_closest_frustum: true,
                    ..Default::default()
                },
                options,
            ));
        }
        if classification {
            let shader = Rc::new(ShaderProgram::from_sources(&context,
                &ViewportQuad::<BrowserGl>::shader_options(ShaderSource {
                    defines: vec!["LOG_DEPTH_READ_ONLY".into()],
                    ..ShaderSource::new("in vec2 v_textureCoordinates;\nvoid main(){float d=czm_unpackDepth(texture(czm_globeDepthTexture,v_textureCoordinates));out_FragColor=vec4(d,d,d,1.0);}")
                })).map_err(error)?);
            let mut options = SceneDrawOptions::new(quad.vertex_array().clone(), shader);
            options.state.depth_mask = false;
            commands.push(SceneCommand::from_draw(
                "classification-depth",
                context.clone(),
                Pass::Cesium3DTileClassification,
                DrawMetadata {
                    bounding_volume: Some(BoundingSphere::new(Cartesian3::new(6378137., 0., 0.), 300.)),
                    execute_in_closest_frustum: true,
                    ..Default::default()
                },
                options,
            ));
        }
        let mut scene = Scene::new(
            RendererSceneBackend::new(context.clone()),
            CameraOptions::new(8., 8.),
            SceneOptions {
                logarithmic_depth_buffer: config["log"] == true,
                high_dynamic_range: config["hdr"] == true,
                ..Default::default()
            },
            JulianDate::new(2451545., 0., TimeStandard::Tai),
        )
        .map_err(error)?;
        scene.msaa_samples = config["samples"].as_u64().unwrap_or(1) as u32;
        scene.camera.position = Cartesian3::new(6379137., 0., 0.);
        scene.camera.direction = -Cartesian3::UNIT_X;
        scene.camera.up = Cartesian3::UNIT_Z;
        scene.camera.right = Cartesian3::UNIT_Y;
        if config["mrt"] == false {
            let mut caps = context.capabilities().map_err(error)?;
            caps.draw_buffers = false;
            scene.view.resources::<BrowserGl>().unwrap().borrow_mut().oit =
                Some(Oit::with_capabilities(caps).map_err(error)?);
        }
        scene.primitives.push(Box::new(Commands(commands)));
        scene.rethrow_render_errors = true;
        scene.render(JulianDate::new(2451545., 0., TimeStandard::Tai)).map_err(error)?;
        let pixels = context.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        let result=json!({"pixels":pixels,"supported":scene.view.resources::<BrowserGl>().unwrap().borrow().oit.as_ref().unwrap().is_supported()}).to_string();
        scene.destroy().map_err(error)?;
        Ok(result)
    }
    #[wasm_bindgen]
    pub fn scene_depth_resources(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use cesium_rs::scene::*;
        let config: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let context = if config["webgl1"] == true {
            Context::new_webgl1(BrowserGl::new_webgl1(gl).map_err(error)?)
        } else {
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?)
        };
        let hdr = config["hdr"] == true;
        let samples = config["samples"].as_u64().unwrap_or(1) as u32;
        let viewport = Viewport { x: 0, y: 0, width: 8, height: 8 };
        let pass = PassState {
            viewport: Some(if config["scissor"] == true {
                Viewport { x: 1, y: 2, width: 5, height: 4 }
            } else {
                viewport
            }),
            ..Default::default()
        };
        let read = |framebuffer: Option<&Framebuffer<BrowserGl>>| -> Result<serde_json::Value, JsValue> {
            let read = context
                .read_pixels_from(ReadState { framebuffer, region: TextureRegion { x: 0, y: 0, width: 8, height: 8 } })
                .map_err(error)?;
            Ok(if read.pixel_datatype == PixelDatatype::Float {
                json!(read.bytes.chunks_exact(4).map(|b| f32::from_ne_bytes(b.try_into().unwrap())).collect::<Vec<_>>())
            } else {
                json!(read.bytes)
            })
        };
        let mut scene = SceneFramebuffer::new().map_err(error)?;
        scene.update(&context, viewport, hdr, samples).map_err(error)?;
        scene.clear(&context, &pass, Color::RED).map_err(error)?;
        scene.prepare_color_textures().map_err(error)?;
        let cleared = read(scene.color_framebuffer().map(std::rc::Rc::as_ref))?;
        let mut globe = GlobeDepth::new().map_err(error)?;
        globe.picking = config["picking"] == true;
        globe.update(&context, &pass, viewport, samples, hdr, config["clear"] == true).map_err(error)?;
        ClearCommand {
            framebuffer: globe.framebuffer().cloned(),
            color: Some([0.2, 0.4, 0.6, 1.]),
            depth: Some(0.3),
            stencil: Some(128),
            ..Default::default()
        }
        .execute(&context, &pass)
        .map_err(error)?;
        globe.execute_copy_depth(&context, &pass).map_err(error)?;
        let packed = read(globe.packed_depth_framebuffer().map(std::rc::Rc::as_ref))?;
        let mut other = SceneFramebuffer::new().map_err(error)?;
        other.update(&context, viewport, false, 1).map_err(error)?;
        let other_pass = PassState { framebuffer: other.framebuffer().cloned(), ..pass.clone() };
        ClearCommand {
            color: Some([0.; 4]),
            depth: Some(config["depth"].as_f64().unwrap_or(0.7)),
            stencil: Some(if config["stencil"] == true { 128 } else { 0 }),
            ..Default::default()
        }
        .execute(&context, &other_pass)
        .map_err(error)?;
        globe.execute_update_depth(&context, &other_pass, other.depth_stencil_texture().cloned()).map_err(error)?;
        let merged = read(globe.packed_depth_framebuffer().map(std::rc::Rc::as_ref))?;
        let mut pick = PickDepth::new().map_err(error)?;
        pick.update(&context, other.depth_stencil_texture().unwrap().clone()).map_err(error)?;
        pick.execute_copy_depth(&context, &pass, globe.packed_depth_texture().map_err(error)?.cloned())
            .map_err(error)?;
        let picked = read(pick.framebuffer().map(std::rc::Rc::as_ref))?;
        let depth = pick.get_depth(&context, 4., 4.).map_err(error)?;
        let mut translucent = GlobeTranslucencyFramebuffer::new().map_err(error)?;
        translucent.update_and_clear(hdr, viewport, &context, &pass).map_err(error)?;
        translucent.clear_classification(&context, &pass).map_err(error)?;
        ClearCommand {
            framebuffer: translucent.classification_framebuffer().cloned(),
            color: Some([1., 0., 0., 0.5]),
            depth: Some(0.4),
            ..Default::default()
        }
        .execute(&context, &pass)
        .map_err(error)?;
        translucent.pack_depth(&context, &pass).map_err(error)?;
        let translucent_depth = read(translucent.packed_depth_framebuffer().map(std::rc::Rc::as_ref))?;
        let mut edge = EdgeFramebuffer::new().map_err(error)?;
        edge.update(
            &context,
            viewport,
            hdr,
            globe.color_texture().map_err(error)?.cloned(),
            globe.depth_stencil_texture().cloned(),
        )
        .map_err(error)?;
        edge.clear(&context, &pass, Some(Color::LIME)).map_err(error)?;
        let edge_pixels = read(edge.framebuffer().map(std::rc::Rc::as_ref))?;
        let result=json!({"cleared":cleared,"packed":packed,"merged":merged,"picked":picked,"depth":depth,"translucent":translucent_depth,"edge":edge_pixels}).to_string();
        edge.destroy().map_err(error)?;
        pick.destroy().map_err(error)?;
        translucent.destroy().map_err(error)?;
        globe.destroy().map_err(error)?;
        other.destroy().map_err(error)?;
        scene.destroy().map_err(error)?;
        context.destroy().map_err(error)?;
        Ok(result)
    }
    #[wasm_bindgen]
    pub async fn scene_pick(gl: JsValue, config: &str, schedule: js_sys::Function) -> Result<String, JsValue> {
        use cesium_rs::scene::*;
        use std::rc::Rc;
        struct Commands(Vec<SceneCommand>);
        impl ScenePrimitive for Commands {
            fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
                frame.command_list.extend(self.0.clone());
                Ok(())
            }
            fn destroy(&mut self) -> Result<(), RuntimeError> {
                Ok(())
            }
        }
        let config: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let webgl1 = config["webgl1"].as_bool().unwrap_or(false);
        let context = if webgl1 {
            Context::new_webgl1(BrowserGl::new_webgl1(gl).map_err(error)?)
        } else {
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?)
        };
        let quad = ViewportQuad::new(&context).map_err(error)?;
        let a = Rc::new("a".to_string());
        let b = Rc::new("b".to_string());
        let c = Rc::new("c".to_string());
        let ids = [a.clone(), b, a, c]
            .map(|o| context.create_pick_id(o).map_err(error))
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let source=ShaderSource::new("uniform vec4 color0;uniform vec4 color1;uniform vec4 color2;uniform vec4 color3;uniform float hole;void main(){out_FragColor=vec4(0.7,0.2,0.1,0.5);if(hole>0.0&&gl_FragCoord.x>3.0&&gl_FragCoord.x<5.0&&gl_FragCoord.y>3.0&&gl_FragCoord.y<5.0){out_FragColor.a=0.0;}}");
        let program = Rc::new(
            ShaderProgram::from_sources(&context, &ViewportQuad::<BrowserGl>::shader_options(source)).map_err(error)?,
        );
        let mut options = SceneDrawOptions::new(quad.vertex_array().clone(), program);
        options.state.blending.enabled = true;
        for (i, id) in ids.iter().enumerate() {
            options.uniforms.push((
                format!("color{i}"),
                UniformValue::Float(vec![
                    id.color.red as f32,
                    id.color.green as f32,
                    id.color.blue as f32,
                    id.color.alpha as f32,
                ]),
            ));
        }
        options.uniforms.push(("hole".into(), UniformValue::Float(vec![if config["hole"] == true { 1. } else { 0. }])));
        let command=SceneCommand::from_draw("quadrants",context.clone(),Pass::Opaque,DrawMetadata{pick_id:Some("(gl_FragCoord.y < 4.0 ? (gl_FragCoord.x < 4.0 ? color0 : color1) : (gl_FragCoord.x < 4.0 ? color2 : color3))".into()),execute_in_closest_frustum:true,..Default::default()},options);
        let overlay = SceneCommand::clear(
            "overlay",
            context.clone(),
            ClearCommand { color: Some([0., 0., 1., 1.]), pass: Some(Pass::Overlay), ..Default::default() },
        );
        let mut scene = Scene::new(
            RendererSceneBackend::new(context.clone()),
            CameraOptions::new(8., 8.),
            SceneOptions { logarithmic_depth_buffer: config["log"] == true, ..Default::default() },
            JulianDate::new(2451545., 0., TimeStandard::Tai),
        )
        .map_err(error)?;
        scene.camera.client_width = 4.;
        scene.camera.client_height = 4.;
        scene.primitives.push(Box::new(Commands(vec![command, overlay])));
        let target = FramebufferManagerOptions { depth_stencil: true, ..Default::default() };
        let mut external = FramebufferManager::new(target).map_err(error)?;
        external.update(&context, 8, 8, None, None, None).map_err(error)?;
        let external = external.framebuffer().unwrap().clone();
        scene.framebuffer = Some(FrameResource::new(external));
        scene.viewport = Some(Viewport { x: 1, y: 1, width: 6, height: 6 });
        scene.scissor_test =
            Some(ScissorTest { enabled: true, rectangle: Viewport { x: 2, y: 2, width: 2, height: 2 } });
        scene.blending_enabled = Some(true);
        let old_target = scene.framebuffer.clone();
        let old_viewport = scene.viewport;
        let old_scissor = scene.scissor_test.clone();
        let mut outputs = Vec::new();
        for q in config["queries"].as_array().unwrap() {
            let p = Cartesian2::new(q[0].as_f64().unwrap(), q[1].as_f64().unwrap());
            let width = q[2].as_f64();
            let height = q[3].as_f64();
            let limit = q[4].as_u64().unwrap() as usize;
            let objects = if config["async"] == true {
                let request = scene.begin_pick_async(p, width, height, limit).map_err(error)?;
                request
                    .wait(|| {
                        let value = schedule.call0(&JsValue::NULL);
                        async move {
                            let value = value.map_err(|_| RuntimeError::new("schedule error"))?;
                            wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&value))
                                .await
                                .map_err(|_| RuntimeError::new("schedule rejection"))?;
                            Ok(())
                        }
                    })
                    .await
                    .map_err(error)?
            } else {
                scene.pick_objects(p, width, height, limit).map_err(error)?
            };
            outputs.push(
                objects
                    .into_iter()
                    .map(|o| o.downcast::<String>().map(|s| s.as_ref().clone()).unwrap())
                    .collect::<Vec<_>>(),
            );
            if scene.framebuffer != old_target
                || scene.viewport != old_viewport
                || scene.scissor_test != old_scissor
                || scene.blending_enabled != Some(true)
            {
                return Err(error("Pick failed to restore Scene pass state"));
            }
        }
        let result = json!({"objects":outputs,"restored":true,"frame":scene.frame_state.frame_number}).to_string();
        scene.destroy().map_err(error)?;
        Ok(result)
    }
    #[wasm_bindgen]
    pub fn scene_mixed_lod(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use cesium_rs::scene::*;
        use std::rc::Rc;
        struct Content(SceneCommand);
        impl TilesetContent for Content {
            fn metrics(&self) -> TileContentMetrics {
                Default::default()
            }
            fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
                Ok(true)
            }
            fn update(&mut self, f: &mut FrameState) -> Result<(), RuntimeError> {
                f.command_list.push(self.0.clone());
                Ok(())
            }
            fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
                Ok(None)
            }
            fn destroy(&mut self) -> Result<(), RuntimeError> {
                Ok(())
            }
        }
        struct Commands(Vec<SceneCommand>);
        impl ScenePrimitive for Commands {
            fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update(&mut self, f: &mut FrameState) -> Result<(), RuntimeError> {
                f.command_list.extend(self.0.clone());
                Ok(())
            }
            fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn destroy(&mut self) -> Result<(), RuntimeError> {
                Ok(())
            }
        }
        let config: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let flag = |n: &str| config[n].as_bool().unwrap_or(false);
        let child_distance = config["distance"].as_f64().unwrap_or(1000.) as f32;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let tile = |radius: f64, geometric_error: f64, children: Option<Vec<serde_json::Value>>| {
            let mut v = json!({"boundingVolume":{"sphere":[6378137,0,0,radius]},"geometricError":geometric_error,"refine":"REPLACE","content":{"uri":format!("{radius}.glb")}});
            if let Some(c) = children {
                v["children"] = json!(c);
            }
            v
        };
        let document = json!({"asset":{"version":"1.1"},"geometricError":1000,"root":tile(300.,500.,Some(vec![tile(100.,100.,Some(vec![tile(30.,0.,None),tile(20.,0.,None)]))]))});
        let mut tileset = Cesium3DTileset::from_json(
            Resource::new("https://example.com/tileset.json").unwrap(),
            &document,
            TilesetOptions {
                skip_level_of_detail: flag("skip"),
                maximum_screen_space_error: 0.1,
                dynamic_screen_space_error: false,
                foveated_screen_space_error: false,
                cull_with_children_bounds: false,
                cull_requests_while_moving: false,
                ..Default::default()
            },
            None,
        )
        .map_err(error)?;
        let shader=Rc::new(ShaderProgram::from_sources(&ctx,&ShaderProgramOptions{vertex_shader_source:ShaderSource::new("in vec2 position;in float distance;void main(){gl_Position=czm_projection*vec4(position*distance*0.5,-distance,1.0);}"),fragment_shader_source:ShaderSource::new("uniform vec4 color;void main(){out_FragColor=color;}"),attribute_locations:std::collections::BTreeMap::from([("position".into(),0),("distance".into(),1)])}).map_err(error)?);
        for id in [0, 2] {
            let size = if id == 0 { 1. } else { 0.5 };
            let mut vertices = Vec::<f32>::new();
            for distance in if id == 0 { vec![800., 1200.] } else { vec![child_distance] } {
                for (x, y) in [(-size, -size), (size, -size), (size, size), (-size, size)] {
                    vertices.extend([x, y, distance]);
                }
            }
            let bytes = floats(&vertices);
            let buffer = Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))
                .map_err(error)?;
            let mut position = VertexAttribute::buffer(0, buffer.clone(), 2);
            position.stride = 12;
            let mut distance = VertexAttribute::buffer(1, buffer, 1);
            distance.offset = 8;
            distance.stride = 12;
            let indices: Vec<u8> =
                if id == 0 { vec![0u16, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6] } else { vec![0u16, 1, 2, 0, 2, 3] }
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect();
            let index = Buffer::new(
                &ctx,
                BufferKind::Index(IndexDatatype::UnsignedShort),
                BufferUsage::StaticDraw,
                BufferData::Bytes(&indices),
            )
            .map_err(error)?;
            let array = Rc::new(VertexArray::new(&ctx, vec![position, distance], Some(index)).map_err(error)?);
            let mut options = SceneDrawOptions::new(array, shader.clone());
            options.state.depth_test.enabled = true;
            options.uniforms.push((
                "color".into(),
                UniformValue::Float(if id == 0 { vec![1., 0., 0., 1.] } else { vec![0., 1., 0., 1.] }),
            ));
            let command = SceneCommand::from_draw(
                id.to_string(),
                ctx.clone(),
                Pass::Cesium3DTile,
                DrawMetadata { bounding_volume: Some(tileset.tiles[id].bounding_sphere), ..Default::default() },
                options,
            );
            tileset.install_content(id, Box::new(Content(command))).map_err(error)?;
        }
        let mut scene = Scene::new(
            RendererSceneBackend::new(ctx.clone()),
            CameraOptions::new(8., 8.),
            SceneOptions { logarithmic_depth_buffer: flag("log"), ..Default::default() },
            JulianDate::new(2451545., 0., TimeStandard::Tai),
        )
        .map_err(error)?;
        scene.msaa_samples = 1;
        scene.camera.position = Cartesian3::new(6379137., 0., 0.);
        scene.camera.direction = Cartesian3::new(-1., 0., 0.);
        scene.camera.up = Cartesian3::UNIT_Z;
        scene.camera.right = Cartesian3::UNIT_Y;
        if flag("preserve") {
            let clear = SceneCommand::clear(
                "classification-bits",
                ctx.clone(),
                ClearCommand {
                    pass: Some(Pass::Globe),
                    stencil: Some(9),
                    render_state: RenderState { stencil_mask: 15, ..Default::default() },
                    ..Default::default()
                },
            );
            let quad = ViewportQuad::new(&ctx).map_err(error)?;
            let program = Rc::new(
                ShaderProgram::from_sources(
                    &ctx,
                    &ViewportQuad::<BrowserGl>::shader_options(ShaderSource {
                        defines: vec!["LOG_DEPTH_READ_ONLY".into()],
                        ..ShaderSource::new("void main(){out_FragColor=vec4(0.0,0.0,1.0,1.0);}")
                    }),
                )
                .map_err(error)?,
            );
            let mut options = SceneDrawOptions::new(quad.vertex_array().clone(), program);
            options.state.stencil_test.enabled = true;
            options.state.stencil_test.reference = 9;
            options.state.stencil_test.mask = 15;
            options.state.stencil_test.front_function = webgl_constants::EQUAL;
            options.state.stencil_test.back_function = webgl_constants::EQUAL;
            let overlay = SceneCommand::from_draw(
                "classification-bit-check",
                ctx.clone(),
                Pass::Opaque,
                DrawMetadata {
                    bounding_volume: Some(tileset.tiles[0].bounding_sphere),
                    execute_in_closest_frustum: true,
                    ..Default::default()
                },
                options,
            );
            scene.primitives.push(Box::new(Commands(vec![clear, overlay])));
        }
        scene.primitives.push(Box::new(tileset));
        scene.rethrow_render_errors = true;
        scene.render(JulianDate::new(2451545., 0., TimeStandard::Tai)).map_err(error)?;
        let pixels = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        Ok(json!({"pixels":pixels,"executed":scene.frame_state.executed_commands.len(),"frustums":scene.view.frustum_commands_list.iter().map(|f|vec![f.near,f.far]).collect::<Vec<_>>()} ).to_string())
    }
    #[wasm_bindgen]
    pub fn scene_draw_variants(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use cesium_rs::scene::*;
        use std::rc::Rc;
        struct Commands(Vec<SceneCommand>);
        impl ScenePrimitive for Commands {
            fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update(&mut self, f: &mut FrameState) -> Result<(), RuntimeError> {
                f.command_list.extend(self.0.clone());
                Ok(())
            }
            fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn destroy(&mut self) -> Result<(), RuntimeError> {
                Ok(())
            }
        }
        let config: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let flag = |n: &str| config[n].as_bool().unwrap_or(false);
        let ctx = if flag("webgl1") {
            Context::new_webgl1(BrowserGl::new_webgl1(gl).map_err(error)?)
        } else {
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?)
        };
        let quad = ViewportQuad::new(&ctx).map_err(error)?;
        let source=ShaderSource::new("uniform sampler2D image;uniform vec4 u_pickColor;void main(){out_FragColor=texture(image,vec2(0.5));\n#ifdef HDR\nout_FragColor.rgb*=0.5;\n#endif\n}");
        let program = Rc::new(
            ShaderProgram::from_sources(&ctx, &ViewportQuad::<BrowserGl>::shader_options(source)).map_err(error)?,
        );
        let mut texture_options = TextureOptions::rgba(1, 1);
        texture_options.flip_y = false;
        texture_options.sampler = Sampler::NEAREST;
        let texture = Rc::new(Texture::new(&ctx, texture_options, Some(&[200, 100, 50, 255])).map_err(error)?);
        let mut options = SceneDrawOptions::new(quad.vertex_array().clone(), program);
        options.textures.push(("image".into(), texture));
        options
            .uniforms
            .push(("u_pickColor".into(), UniformValue::Float(vec![25. / 255., 50. / 255., 75. / 255., 1.])));
        let mut command = SceneCommand::from_draw(
            "quad",
            ctx.clone(),
            Pass::Opaque,
            DrawMetadata {
                pick_id: Some("u_pickColor".into()),
                execute_in_closest_frustum: true,
                ..Default::default()
            },
            options,
        );
        let mut scene = Scene::new(
            RendererSceneBackend::new(ctx.clone()),
            CameraOptions::new(8., 8.),
            SceneOptions {
                logarithmic_depth_buffer: flag("log"),
                high_dynamic_range: flag("hdr"),
                // This case deliberately hides OES_texture_float to exercise
                // byte clipping textures. Disable the unrelated float OIT path.
                order_independent_translucency: !(flag("clip") && flag("webgl1")),
                ..Default::default()
            },
            JulianDate::new(2451545., 0., TimeStandard::Tai),
        )
        .map_err(error)?;
        let planes = if flag("clip") {
            let mut planes = ClippingPlaneCollection::new([
                ClippingPlane::handle(Cartesian3::UNIT_X, 0.),
                ClippingPlane::handle(Cartesian3::UNIT_Y, 0.),
            ]);
            planes.union_clipping_regions = flag("union");
            planes.edge_color = Color::RED;
            planes.edge_width = 1.25;
            let planes = Rc::new(std::cell::RefCell::new(planes));
            command.clipping_planes =
                Some(SceneClippingPlanes::new(planes.clone(), scene.camera.inverse_view_matrix()));
            Some(planes)
        } else {
            None
        };
        scene.primitives.push(Box::new(Commands(vec![command])));
        let target = if flag("target") {
            let color = Rc::new(Texture::new(&ctx, TextureOptions::rgba(8, 8), None).map_err(error)?);
            let target = Rc::new(
                Framebuffer::new(
                    &ctx,
                    FramebufferOptions { colors: vec![Attachment::Texture(color)], ..Default::default() },
                )
                .map_err(error)?,
            );
            scene.framebuffer = Some(FrameResource::new(Rc::clone(&target)));
            Some(target)
        } else {
            None
        };
        scene.viewport = Some(Viewport { x: 0, y: 0, width: 8, height: 8 });
        let passes = FramePasses {
            render: !flag("pick") && !flag("depth"),
            pick: flag("pick"),
            depth: flag("depth"),
            ..Default::default()
        };
        if let Some(planes) = planes {
            let mut phases = Vec::new();
            for phase in 0..6 {
                {
                    let mut p = planes.borrow_mut();
                    match phase {
                        1 => p.get(0).unwrap().borrow_mut().set_distance(0.25),
                        2 => {
                            p.get(0).unwrap().borrow_mut().set_distance(-0.25);
                            p.get(1).unwrap().borrow_mut().set_distance(0.25);
                        }
                        3 => {
                            let plane = p.get(1).unwrap().clone();
                            p.remove(&plane);
                        }
                        4 => p.remove_all(),
                        5 => p.add(ClippingPlane::handle(Cartesian3::UNIT_Y, -0.1)),
                        _ => {}
                    }
                }
                scene.draw_passes(passes).map_err(error)?;
                let bytes = ctx
                    .read_pixels_from(ReadState {
                        framebuffer: target.as_deref(),
                        region: TextureRegion { x: 0, y: 0, width: 8, height: 8 },
                    })
                    .map_err(error)?
                    .bytes;
                let texture = planes.borrow().texture::<BrowserGl>();
                phases.push(json!({"pixels":bytes,"texture":texture.map(|t|[t.width(),t.height()]),"float":ctx.capabilities().map_err(error)?.floating_point_texture}));
            }
            return Ok(json!(phases).to_string());
        }
        scene.draw_passes(passes).map_err(error)?;
        // Repeat to exercise variant reuse and all retained GPU input lifetimes.
        scene.draw_passes(passes).map_err(error)?;
        let bytes = ctx
            .read_pixels_from(ReadState {
                framebuffer: target.as_deref(),
                region: TextureRegion { x: 4, y: 4, width: 1, height: 1 },
            })
            .map_err(error)?
            .bytes;
        Ok(json!({"pixels":bytes,"executed":scene.frame_state.executed_commands.len()}).to_string())
    }
    #[wasm_bindgen]
    pub fn renderer_buffers(gl: JsValue) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let b = Buffer::new(
            &ctx,
            BufferKind::Vertex,
            BufferUsage::DynamicDraw,
            BufferData::Bytes(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
        )
        .map_err(error)?;
        b.copy_from_array_view(&[21, 22, 23], 3).map_err(error)?;
        let target =
            Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(12)).map_err(error)?;
        target.copy_from_buffer(&b, 2, 4, 6).map_err(error)?;
        let indices = Buffer::new(
            &ctx,
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&[0, 0, 1, 0, 2, 0]),
        )
        .map_err(error)?;
        let mut attr = VertexAttribute::buffer(0, b.clone(), 3);
        attr.datatype = ComponentDatatype::UnsignedByte;
        attr.stride = 4;
        let va = VertexArray::new(
            &ctx,
            vec![attr, VertexAttribute::constant(1, vec![1., 0., 0., 1.])],
            Some(indices.clone()),
        )
        .map_err(error)?;
        let mut kinds = vec![];
        for kind in [
            BufferKind::Vertex,
            BufferKind::Index(IndexDatatype::UnsignedByte),
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferKind::Index(IndexDatatype::UnsignedInt),
            BufferKind::PixelPack,
        ] {
            let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
            let tmp = Buffer::new(&ctx, kind, BufferUsage::DynamicRead, BufferData::Bytes(&bytes)).map_err(error)?;
            tmp.copy_from_buffer(&tmp, 0, 8, 4).map_err(error)?;
            kinds.push(json!([
                tmp.size_in_bytes(),
                tmp.number_of_indices(),
                tmp.get_buffer_data(0, 12).map_err(error)?
            ]));
        }
        let result = json!({"kinds":kinds,"source":b.get_buffer_data(0,12).map_err(error)?, "target":target.get_buffer_data(0,12).map_err(error)?, "slice":b.get_buffer_data(3,4).map_err(error)?, "vertices":va.number_of_vertices(), "indices":indices.number_of_indices(), "bytes":b.size_in_bytes(), "usage":b.usage() as u32});
        // Bounds checks must reject without a partial write; copies may share
        // the same allocation only when their byte ranges do not overlap.
        assert!(b.copy_from_array_view(&[1, 2, 3], 11).is_err());
        assert!(b.copy_from_buffer(&b, 0, 1, 4).is_err());
        assert!(target.copy_from_buffer(&indices, 0, 0, 2).is_err());
        let alias = b.clone();
        va.destroy().map_err(error)?;
        assert!(!b.is_destroyed());
        b.destroy().map_err(error)?;
        assert!(alias.is_destroyed());
        assert!(alias.get_buffer_data(0, 1).is_err());
        Ok(result.to_string())
    }
    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    #[wasm_bindgen]
    pub fn renderer_state(gl: JsValue, state: &str, pass: &str) -> Result<(), JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let state: RenderState = serde_json::from_str(state).map_err(error)?;
        let pass: PassState<BrowserGl> = serde_json::from_str(pass).map_err(error)?;
        state.apply(&ctx, &pass, ctx.host().drawing_buffer_size().map_err(error)?).map_err(error)
    }
    #[wasm_bindgen]
    pub fn renderer_draw(gl: JsValue, config: &str, image: Option<JsValue>) -> Result<String, JsValue> {
        let c: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let flag = |name: &str| c[name].as_bool().unwrap_or(false);
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let indexed = flag("indexed");
        let instanced = flag("instanced");
        let textured = flag("textured");
        let positions: &[f32] = if indexed {
            &[-1., -1., 1., -1., 1., 1., -1., 1.]
        } else {
            &[-1., -1., -1., -1., -1., -1., -1., -1., 1., -1., 1., 1., -1., -1., 1., 1., -1., 1.]
        };
        let vertex =
            Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&floats(positions)))
                .map_err(error)?;
        let index = if indexed {
            Some(
                Buffer::new(
                    &ctx,
                    BufferKind::Index(IndexDatatype::UnsignedShort),
                    BufferUsage::StaticDraw,
                    BufferData::Bytes(&[0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0, 0, 2, 0, 3, 0]),
                )
                .map_err(error)?,
            )
        } else {
            None
        };
        let mut attributes =
            vec![VertexAttribute::buffer(0, vertex, 2), VertexAttribute::constant(1, vec![1., 0.5, 0.25, 1.])];
        if instanced {
            let offsets = Buffer::new(
                &ctx,
                BufferKind::Vertex,
                BufferUsage::StaticDraw,
                BufferData::Bytes(&floats(&[-0.5, 0., 0.5, 0.])),
            )
            .map_err(error)?;
            let mut a = VertexAttribute::buffer(2, offsets, 2);
            a.instance_divisor = 1;
            attributes.push(a);
        } else {
            attributes.push(VertexAttribute::constant(2, vec![0., 0.]));
        }
        let va = VertexArray::new(&ctx, attributes, index).map_err(error)?;
        let vs = format!("#version 300 es\nprecision highp float;\nin vec2 position; in vec4 color; in vec2 offset; out vec4 v_color; out vec2 v_uv; uniform mat4 transform; void main() {{ gl_Position = transform * vec4(position * vec2({}, 1.0) + offset,0.,1.); v_color=color; v_uv=position*.5+.5; }}",if instanced {"0.5"} else {"1.0"});
        let fs = format!("#version 300 es\nprecision highp float;\nin vec4 v_color; in vec2 v_uv; uniform vec4 tint; uniform int enabled; {}out vec4 result; void main() {{result= (enabled==1 ? tint : vec4(0.)) * v_color {};}}", if textured {"uniform sampler2D image; "} else {""},if textured {if c["lod"].as_u64()==Some(1){"* textureLod(image,v_uv,1.0)"}else if c["lod"].as_u64()==Some(2){"* textureLod(image,v_uv,2.0)"}else{"* texture(image,v_uv)"}} else {""});
        let program =
            ShaderProgram::new(&ctx, &vs, &fs, &[("position", 0), ("color", 1), ("offset", 2)]).map_err(error)?;
        assert_eq!(program.vertex_attribute_location("position").map_err(error)?, 0);
        let texture = if textured {
            let mut opts = TextureOptions::rgba(2, 2);
            opts.flip_y = c["flipY"].as_bool().unwrap_or(true);
            opts.sampler = if flag("linear") { Sampler::default() } else { Sampler::NEAREST };
            if let Some(v) = c["textureFormat"].as_u64() {
                opts.pixel_format = PixelFormat::from_value(v as u32).unwrap();
            }
            if let Some(v) = c["textureDatatype"].as_u64() {
                opts.pixel_datatype = PixelDatatype::from_value(v as u32).unwrap();
            }
            if let Some(v) = c["textureWidth"].as_u64() {
                opts.width = v as u32;
            }
            if let Some(v) = c["textureHeight"].as_u64() {
                opts.height = v as u32;
            }
            let pixels = c
                .get("pixels")
                .map(|v| v.as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect::<Vec<_>>())
                .unwrap_or(vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]);
            opts.pre_multiply_alpha = flag("preMultiplyAlpha");
            opts.skip_color_space_conversion = flag("skipColorSpaceConversion");
            if flag("samplerMips") {
                opts.sampler.minification_filter = TextureMinificationFilter::NearestMipmapNearest;
            }
            let mips: Vec<Vec<u8>> = c
                .get("mipLevels")
                .map(|v| {
                    v.as_array()
                        .unwrap()
                        .iter()
                        .map(|a| a.as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect())
                        .collect()
                })
                .unwrap_or_default();
            let t = match c["sourceKind"].as_str() {
                Some("image") => Texture::from_image(&ctx, opts, image.clone().unwrap()).map_err(error)?,
                Some("imageSub") => {
                    let t = Texture::new(&ctx, opts, None).map_err(error)?;
                    t.copy_from_image(
                        image.clone().unwrap(),
                        TextureRegion { x: 0, y: 0, width: t.width(), height: t.height() },
                    )
                    .map_err(error)?;
                    t
                }
                Some("compressed") => Texture::compressed(&ctx, opts, &pixels, &mips).map_err(error)?,
                Some("mipmaps") => Texture::with_mipmaps(&ctx, opts, &pixels, &mips).map_err(error)?,
                _ => Texture::new(&ctx, opts, Some(&pixels)).map_err(error)?,
            };
            if flag("update") {
                t.copy_from(&[255, 255, 0, 255], TextureRegion { x: 1, y: 0, width: 1, height: 1 }).map_err(error)?;
            }
            if flag("mipmap") {
                t.generate_mipmap(MipmapHint::DontCare).map_err(error)?;
            }
            Some(t)
        } else {
            None
        };
        let pass: PassState<BrowserGl> =
            serde_json::from_value(c.get("pass").cloned().unwrap_or(json!({}))).map_err(error)?;
        let state: RenderState = serde_json::from_value(c.get("state").cloned().unwrap_or(json!({}))).map_err(error)?;
        ClearCommand { color: Some([0.1, 0.2, 0.3, 1.]), depth: Some(1.), stencil: Some(0), ..Default::default() }
            .execute(&ctx, &PassState::default())
            .map_err(error)?;
        let mut command = DrawCommand::new(&va, &program);
        command.render_state = state;
        command.offset = c["offset"].as_u64().unwrap_or(0) as usize;
        command.count = c["count"].as_u64().map(|v| v as usize);
        command.instance_count = if instanced { 2 } else { 0 };
        command.uniforms = vec![
            ("tint".into(), UniformValue::Float(vec![1., 1., 1., 0.5])),
            ("enabled".into(), UniformValue::Int(vec![1])),
            (
                "transform".into(),
                UniformValue::Matrix {
                    dimension: 4,
                    values: vec![1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.],
                },
            ),
        ];
        if let Some(t) = &texture {
            command.textures.push(("image".into(), t));
        }
        command.execute(&ctx, &pass).map_err(error)?;
        // Another VAO with a different constant must not permanently change
        // the first one's color (constants are global WebGL context state).
        if flag("switchConstant") {
            let other = VertexArray::new(
                &ctx,
                vec![
                    VertexAttribute::constant(0, vec![0., 0.]),
                    VertexAttribute::constant(1, vec![0., 1., 0., 1.]),
                    VertexAttribute::constant(2, vec![0., 0.]),
                ],
                None,
            )
            .map_err(error)?;
            DrawCommand::new(&other, &program).execute(&ctx, &pass).map_err(error)?;
            command.execute(&ctx, &pass).map_err(error)?;
        }
        let pixels = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        Ok(json!({"pixels":pixels,"textureBytes":texture.as_ref().map(Texture::size_in_bytes)}).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_clear(gl: JsValue, state: &str, pass: &str) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let state: RenderState = serde_json::from_str(state).map_err(error)?;
        let pass: PassState<BrowserGl> = serde_json::from_str(pass).map_err(error)?;
        ClearCommand { color: Some([0.1, 0.2, 0.3, 1.]), ..Default::default() }
            .execute(&ctx, &PassState::default())
            .map_err(error)?;
        ClearCommand {
            color: Some([1., 0.5, 0., 0.25]),
            depth: Some(0.5),
            stencil: Some(3),
            render_state: state,
            ..Default::default()
        }
        .execute(&ctx, &pass)
        .map_err(error)?;
        Ok(serde_json::to_string(&ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?)
            .unwrap())
    }

    #[wasm_bindgen]
    pub fn renderer_readback(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use std::rc::Rc;
        let c: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut options = TextureOptions::rgba(3, 2);
        options.pixel_format = PixelFormat::from_value(c["format"].as_u64().unwrap() as u32).unwrap();
        options.pixel_datatype = PixelDatatype::from_value(c["datatype"].as_u64().unwrap() as u32).unwrap();
        options.flip_y = false;
        options.sampler = Sampler::NEAREST;
        let bytes: Vec<u8> = serde_json::from_value(c["bytes"].clone()).map_err(error)?;
        let texture = Rc::new(Texture::new(&ctx, options, Some(&bytes)).map_err(error)?);
        let framebuffer = Framebuffer::new(
            &ctx,
            FramebufferOptions { colors: vec![Attachment::Texture(texture)], ..Default::default() },
        )
        .map_err(error)?;
        let read =
            || ReadState { region: TextureRegion { x: 0, y: 0, width: 3, height: 2 }, framebuffer: Some(&framebuffer) };
        let pixels = ctx.read_pixels_from(read()).map_err(error)?;
        let pbo = ctx.read_pixels_to_pbo(read()).map_err(error)?;
        Ok(json!({"bytes":pixels.bytes,"format":pixels.pixel_format as u32,"datatype":pixels.pixel_datatype as u32,
            "pbo":pbo.get_buffer_data(0,pbo.size_in_bytes()).map_err(error)?,"status":framebuffer.status().map_err(error)?}).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_anisotropy(gl: JsValue, maximum: f64) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut options = TextureOptions::rgba(1, 1);
        options.sampler.maximum_anisotropy = maximum;
        let texture = Texture::new(&ctx, options, None).map_err(error)?;
        texture.bind(0).map_err(error)?;
        let applied = match ctx
            .host()
            .invoke("getTexParameter", &[webgl_constants::TEXTURE_2D.into(), 0x84FE_u32.into()])
            .map_err(error)?
        {
            GlValue::Number(n) => n,
            _ => return Err(error("Missing anisotropy value")),
        };
        Ok(json!([texture.sampler().maximum_anisotropy, applied]).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_manager(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use std::rc::Rc;
        let c: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let flag = |s: &str| c[s].as_bool().unwrap_or(false);
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut manager = FramebufferManager::new(FramebufferManagerOptions {
            color: !flag("depthOnly"),
            depth: flag("depth"),
            depth_stencil: flag("depthStencil"),
            supports_depth_texture: flag("depthTexture"),
            color_attachments_length: c["colors"].as_u64().unwrap_or(1) as usize,
            ..Default::default()
        })
        .map_err(error)?;
        let samples = c["samples"].as_u64().unwrap_or(1) as u32;
        let mut snapshots = vec![];
        for (width, height) in [(4, 2), (3, 4)] {
            manager.update(&ctx, width, height, Some(samples), None, None).map_err(error)?;
            let f = manager.framebuffer().unwrap().clone();
            let unchanged = !manager.update(&ctx, width, height, Some(samples), None, None).map_err(error)?
                && Rc::ptr_eq(&f, manager.framebuffer().unwrap());
            let clear = ClearCommand {
                color: (!flag("depthOnly")).then_some([0.25, 0.5, 0.75, 1.]),
                depth: Some(0.5),
                stencil: Some(3),
                ..Default::default()
            };
            manager.clear(&ctx, &clear, &PassState::default()).map_err(error)?;
            manager.prepare_textures(true).map_err(error)?;
            let mut pixels = vec![];
            let mut metadata = vec![];
            if !flag("depthOnly") {
                for i in 0..c["colors"].as_u64().unwrap_or(1) as usize {
                    let texture = manager.color_texture(i).map_err(error)?.unwrap().clone();
                    metadata.push(json!([
                        texture.width(),
                        texture.height(),
                        texture.pixel_format() as u32,
                        texture.pixel_datatype() as u32,
                        texture.sampler().minification_filter as u32,
                        texture.sampler().magnification_filter as u32
                    ]));
                    let read = Framebuffer::new(
                        &ctx,
                        FramebufferOptions {
                            colors: vec![Attachment::Texture(texture)],
                            destroy_attachments: false,
                            ..Default::default()
                        },
                    )
                    .map_err(error)?;
                    pixels.push(
                        ctx.read_pixels_from(ReadState {
                            region: TextureRegion { x: 0, y: 0, width, height },
                            framebuffer: Some(&read),
                        })
                        .map_err(error)?
                        .bytes,
                    );
                }
            }
            snapshots.push(json!({"status":manager.status().map_err(error)?,"samples":manager.num_samples(),"depth":f.has_depth_attachment(),"depthTexture":manager.depth_texture().is_some(),"depthBuffer":manager.depth_renderbuffer().is_some(),"depthStencilTexture":manager.depth_stencil_texture().is_some(),"depthStencilBuffer":manager.depth_stencil_renderbuffer().is_some(),"unchanged":unchanged,"dirty":manager.is_dirty(width,height,Some(samples),None,None),"preservedClear":clear.framebuffer.is_none(),"metadata":metadata,"pixels":pixels}));
        }
        let retained = manager.color_texture(0).map_err(error)?.cloned();
        manager.destroy().map_err(error)?;
        Ok(json!({"snapshots":snapshots,"destroyed":retained.map(|t|t.is_destroyed()),"framebufferRemoved":manager.framebuffer().is_none()}).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_manager_review(gl: JsValue, mode: u32) -> Result<String, JsValue> {
        use std::rc::Rc;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let make = || Texture::new(&ctx, TextureOptions::rgba(4, 2), None).map(Rc::new).map_err(error);
        let read = |texture: Rc<Texture<BrowserGl>>| -> Result<Vec<u8>, JsValue> {
            let f = Framebuffer::new(
                &ctx,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(texture)],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?;
            Ok(ctx
                .read_pixels_from(ReadState {
                    region: TextureRegion { x: 0, y: 0, width: 4, height: 2 },
                    framebuffer: Some(&f),
                })
                .map_err(error)?
                .bytes)
        };
        if mode < 2 {
            let mut manager = FramebufferManager::new(FramebufferManagerOptions {
                depth: true,
                supports_depth_texture: mode == 1,
                ..Default::default()
            })
            .map_err(error)?;
            manager.update(&ctx, 4, 2, None, None, None).map_err(error)?;
            let original = manager.framebuffer().unwrap().clone();
            let result = manager.update(&ctx, 4, 2, Some(4), None, None);
            let rejected = result.err().is_some_and(|e| e.message.contains("use depth_stencil"));
            manager
                .clear(
                    &ctx,
                    &ClearCommand { color: Some([1., 0., 0., 1.]), depth: Some(0.5), ..Default::default() },
                    &PassState::default(),
                )
                .map_err(error)?;
            manager.prepare_textures(false).map_err(error)?;
            return Ok(json!({"rejected":rejected,"sameFramebuffer":Rc::ptr_eq(&original,manager.framebuffer().unwrap()),"samples":manager.num_samples(),"depth":manager.framebuffer().unwrap().has_depth_attachment(),"pixels":read(manager.color_texture(0).map_err(error)?.unwrap().clone())?}).to_string());
        }
        let a = make()?;
        let b = make()?;
        let replacement = make()?;
        let depth = Rc::new(
            Renderbuffer::new(
                &ctx,
                RenderbufferOptions {
                    format: RenderbufferFormat::DepthComponent16,
                    width: Some(4),
                    height: Some(2),
                    num_samples: 1,
                },
            )
            .map_err(error)?,
        );
        let mut manager = FramebufferManager::new(FramebufferManagerOptions {
            color_attachments_length: 2,
            depth: true,
            create_color_attachments: false,
            create_depth_attachments: false,
            ..Default::default()
        })
        .map_err(error)?;
        manager.set_color_texture(0, Some(a.clone())).map_err(error)?;
        manager.set_color_texture(1, Some(b.clone())).map_err(error)?;
        manager.set_depth_renderbuffer(Some(depth.clone())).map_err(error)?;
        manager.update(&ctx, 4, 2, None, None, None).map_err(error)?;
        let original = manager.framebuffer().unwrap().clone();
        manager
            .clear(&ctx, &ClearCommand { color: Some([1., 0., 0., 1.]), ..Default::default() }, &PassState::default())
            .map_err(error)?;
        manager.set_color_texture(0, Some(replacement.clone())).map_err(error)?;
        manager.set_color_texture(1, Some(b.clone())).map_err(error)?;
        manager.set_depth_renderbuffer(Some(depth)).map_err(error)?;
        let dirty = manager.is_dirty(4, 2, None, None, None);
        let rebuilt = manager.update(&ctx, 4, 2, None, None, None).map_err(error)?;
        manager
            .clear(&ctx, &ClearCommand { color: Some([0., 0., 1., 1.]), ..Default::default() }, &PassState::default())
            .map_err(error)?;
        Ok(json!({"dirty":dirty,"rebuilt":rebuilt,"newFramebuffer":!Rc::ptr_eq(&original,manager.framebuffer().unwrap()),"sameGetterAttachment":Rc::ptr_eq(manager.framebuffer().unwrap().color_texture(0).unwrap(),&replacement),"pixels":[read(a)?,read(replacement)?,read(b)?]}).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_compute(gl: JsValue, config: &str) -> Result<String, JsValue> {
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        let c: serde_json::Value = serde_json::from_str(config).map_err(error)?;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let persists = c["persists"].as_bool().unwrap_or(false);
        let custom = c["custom"].as_bool().unwrap_or(false);
        let fragment=ShaderSource {sources:vec!["in vec2 v_textureCoordinates;uniform sampler2D inputTexture;uniform float scale;void main(){if(v_textureCoordinates.x<0.5){discard;}out_FragColor=texture(inputTexture,v_textureCoordinates)*scale;}".into()],..Default::default()};
        let mut cache = ShaderCache::new(&ctx);
        let program = if custom {
            Some(
                cache
                    .get_shader_program(&ViewportQuad::<BrowserGl>::shader_options(fragment.clone()))
                    .map_err(error)?,
            )
        } else {
            None
        };
        let quad = if custom { Some(ViewportQuad::new(&ctx).map_err(error)?) } else { None };
        let mut input_options = TextureOptions::rgba(1, 1);
        input_options.sampler = Sampler::NEAREST;
        let input = Rc::new(Texture::new(&ctx, input_options, Some(&[128, 64, 32, 255])).map_err(error)?);
        let outputs: Vec<_> = [(4, 2), (2, 3)]
            .into_iter()
            .map(|(w, h)| Texture::new(&ctx, TextureOptions::rgba(w, h), None).map(Rc::new))
            .collect::<Result<_, _>>()
            .map_err(error)?;
        let index = Rc::new(Cell::new(0));
        let events = Rc::new(RefCell::new(vec![]));
        let pre_index = index.clone();
        let pre_events = events.clone();
        let pre_outputs = outputs.clone();
        let mut command = ComputeCommand {
            persists,
            owner: Some("compute".into()),
            fragment_shader_source: Some(fragment),
            shader_program: program.clone(),
            vertex_array: quad.as_ref().map(|q| q.vertex_array().clone()),
            textures: vec![("inputTexture".into(), input)],
            pre_execute: Some(Rc::new(move |command| {
                let i = pre_index.get();
                pre_events.borrow_mut().push(format!("pre{i}"));
                command.output_texture = Some(pre_outputs[i].clone());
                Ok(())
            })),
            ..Default::default()
        };
        let uniform_index = index.clone();
        let uniform_events = events.clone();
        command.uniform_map = Some(Rc::new(move || {
            let i = uniform_index.get();
            uniform_events.borrow_mut().push(format!("uniform{i}"));
            Ok(vec![("scale".into(), UniformValue::Float(vec![if i == 0 { 0.5 } else { 1. }]))])
        }));
        let post_index = index.clone();
        let post_events = events.clone();
        command.post_execute = Some(Box::new(move |_| {
            post_events.borrow_mut().push(format!("post{}", post_index.get()));
            Ok(())
        }));
        let cancel_events = events.clone();
        command.canceled = Some(Box::new(move || {
            cancel_events.borrow_mut().push("cancel".into());
            Ok(())
        }));
        let mut engine = ComputeEngine::new(&ctx);
        let mut pixels = vec![];
        for i in 0..if custom && !persists { 1 } else { 2 } {
            index.set(i);
            command.execute(&mut engine).map_err(error)?;
            let f = Framebuffer::new(
                &ctx,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(outputs[i].clone())],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?;
            pixels.push(
                ctx.read_pixels_from(ReadState {
                    region: TextureRegion { x: 0, y: 0, width: outputs[i].width(), height: outputs[i].height() },
                    framebuffer: Some(&f),
                })
                .map_err(error)?
                .bytes,
            );
        }
        command.cancel().map_err(error)?;
        engine.destroy().map_err(error)?;
        let result = json!({"pixels":pixels,"events":events.borrow().clone(),"programDestroyed":program.map(|p|p.is_destroyed()),"vertexArrayDestroyed":quad.map(|q|q.is_destroyed()),"outputsAlive":outputs.iter().all(|t|!t.is_destroyed()),"engineDestroyed":engine.is_destroyed(),"pass":command.pass() as u32});
        Ok(result.to_string())
    }

    #[wasm_bindgen]
    pub async fn renderer_sync(gl: JsValue, schedule: js_sys::Function) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let sync = Sync::new(&ctx).map_err(error)?;
        ctx.host().invoke("flush", &[]).map_err(error)?;
        sync.wait_for_signal(
            || {
                let schedule = schedule.clone();
                async move {
                    let result =
                        schedule.call0(&JsValue::NULL).map_err(|e| RuntimeError::new(format!("Scheduler: {e:?}")))?;
                    wasm_bindgen_futures::JsFuture::from(js_sys::Promise::resolve(&result))
                        .await
                        .map_err(|e| RuntimeError::new(format!("Scheduler: {e:?}")))?;
                    Ok(())
                }
            },
            Some(100),
        )
        .await
        .map_err(error)?;
        let status = sync.status().map_err(error)?;
        sync.destroy().map_err(error)?;
        Ok(json!([status, sync.is_destroyed()]).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_framebuffers(gl: JsValue, mode: u32) -> Result<String, JsValue> {
        use std::rc::Rc;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let make = || -> Result<Rc<Framebuffer<BrowserGl>>, JsValue> {
            let mut options = TextureOptions::rgba(4, 4);
            options.sampler = Sampler::NEAREST;
            let t = Rc::new(Texture::new(&ctx, options, None).map_err(error)?);
            Ok(Rc::new(
                Framebuffer::new(
                    &ctx,
                    FramebufferOptions {
                        colors: vec![Attachment::Texture(t)],
                        destroy_attachments: false,
                        ..Default::default()
                    },
                )
                .map_err(error)?,
            ))
        };
        let a = make()?;
        let b = make()?;
        let mut pass = PassState::default();
        pass.framebuffer = Some(a.clone());
        ClearCommand { color: Some([1., 0., 0., 1.]), ..Default::default() }.execute(&ctx, &pass).map_err(error)?;
        ClearCommand { color: Some([0., 1., 0., 1.]), framebuffer: Some(b.clone()), ..Default::default() }
            .execute(&ctx, &pass)
            .map_err(error)?;
        let mut result = json!({"status":a.status().map_err(error)?,"count":a.number_of_color_attachments(),"depth":a.has_depth_attachment()});
        let region = TextureRegion { x: 0, y: 0, width: 4, height: 4 };
        let pixels = |f: &Framebuffer<BrowserGl>| {
            ctx.read_pixels_from(ReadState { region, framebuffer: Some(f) }).map_err(error).map(|p| p.bytes)
        };
        result["a"] = json!(pixels(&a)?);
        result["b"] = json!(pixels(&b)?);
        let pbo = ctx.read_pixels_to_pbo(ReadState { region, framebuffer: Some(&b) }).map_err(error)?;
        result["pbo"] = json!(pbo.get_buffer_data(0, 64).map_err(error)?);
        let copied = Rc::new(
            Texture::from_framebuffer(
                &ctx,
                TextureOptions::rgba(4, 4),
                FramebufferCopy { framebuffer: Some(&a), source: region, destination: [0, 0] },
            )
            .map_err(error)?,
        );
        let cf = Framebuffer::new(
            &ctx,
            FramebufferOptions { colors: vec![Attachment::Texture(copied)], ..Default::default() },
        )
        .map_err(error)?;
        result["copy"] = json!(pixels(&cf)?);
        if mode > 0 {
            let color = Rc::new(
                Renderbuffer::new(
                    &ctx,
                    RenderbufferOptions {
                        format: RenderbufferFormat::Rgba8,
                        width: Some(4),
                        height: Some(4),
                        num_samples: if mode == 2 { 4 } else { 1 },
                    },
                )
                .map_err(error)?,
            );
            let depth = Rc::new(
                Renderbuffer::new(
                    &ctx,
                    RenderbufferOptions {
                        format: RenderbufferFormat::DepthComponent16,
                        width: Some(4),
                        height: Some(4),
                        num_samples: if mode == 2 { 4 } else { 1 },
                    },
                )
                .map_err(error)?,
            );
            let f = Rc::new(
                Framebuffer::new(
                    &ctx,
                    FramebufferOptions {
                        colors: vec![Attachment::Renderbuffer(color.clone())],
                        depth: Some(Attachment::Renderbuffer(depth)),
                        ..Default::default()
                    },
                )
                .map_err(error)?,
            );
            ClearCommand { color: Some([0.2, 0.4, 0.6, 1.]), framebuffer: Some(f.clone()), ..Default::default() }
                .execute(&ctx, &PassState::default())
                .map_err(error)?;
            if mode == 2 {
                let multisample = MultisampleFramebuffer::new(
                    &ctx,
                    MultisampleFramebufferOptions {
                        width: 4,
                        height: 4,
                        color_textures: vec![b.color_texture(0).unwrap().clone()],
                        color_renderbuffers: vec![color.clone()],
                        depth_stencil_texture: None,
                        depth_stencil_renderbuffer: None,
                        destroy_attachments: false,
                    },
                )
                .map_err(error)?;
                multisample.blit_framebuffers(false).map_err(error)?;
                multisample.destroy().map_err(error)?;
            } else {
                f.blit_to(&b, webgl_constants::COLOR_BUFFER_BIT).map_err(error)?;
            }
            result["resolved"] = json!(pixels(&b)?);
            result["depth"] = json!(f.has_depth_attachment());
            result["status"] = json!(f.status().map_err(error)?);
            result["renderbuffer"] = json!([color.format() as u32, color.width(), color.height()]);
            f.destroy().map_err(error)?;
            assert!(color.is_destroyed());
        }
        let t = a.color_texture(0).unwrap().clone();
        a.destroy().map_err(error)?;
        assert!(!t.is_destroyed());
        Ok(result.to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_geometry(gl: JsValue, interleave: bool) -> Result<String, JsValue> {
        use std::{
            collections::{BTreeMap, HashMap},
            rc::Rc,
        };
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut attributes = GeometryAttributes::default();
        attributes.position = Some(GeometryAttribute::doubles(2, vec![-1., -1., 1., -1., 1., 1., -1., 1.]));
        let mut color = GeometryAttribute::new(
            ComponentDatatype::UnsignedByte,
            4,
            TypedArray::Uint8(vec![255, 128, 64, 255].repeat(4)),
        );
        color.normalize = true;
        attributes.color = Some(color);
        attributes.others.push((
            "weight".into(),
            GeometryAttribute::new(
                ComponentDatatype::UnsignedShort,
                2,
                TypedArray::Uint16(vec![1, 2, 3, 4, 5, 6, 7, 8]),
            ),
        ));
        let geometry =
            Geometry::new(attributes, Some(TypedArray::Uint16(vec![0, 1, 2, 0, 2, 3])), PrimitiveType::Triangles);
        let va = VertexArray::from_geometry(
            &ctx,
            &geometry,
            GeometryUploadOptions {
                interleave,
                attribute_locations: HashMap::from([("position".into(), 0), ("color".into(), 1), ("weight".into(), 2)]),
                ..Default::default()
            },
        )
        .map_err(error)?;
        let mut layouts = vec![];
        for a in va.attributes() {
            if let AttributeSource::Buffer(b) = &a.source {
                layouts.push(json!([
                    a.index,
                    a.components,
                    a.datatype as u32,
                    a.normalize,
                    a.offset,
                    a.stride,
                    b.get_buffer_data(0, b.size_in_bytes()).map_err(error)?
                ]));
            }
        }
        let mut cache = ShaderCache::new(&ctx);
        let options=ShaderProgramOptions{vertex_shader_source:ShaderSource::new("in vec2 position;in vec4 color;out vec4 v_color;void main(){gl_Position=czm_modelViewProjection*vec4(position,0.,1.);v_color=color;}"),fragment_shader_source:ShaderSource::new("in vec4 v_color;void main(){out_FragColor=v_color;}"),attribute_locations:BTreeMap::from([("position".into(),0),("color".into(),1)])};
        let p = cache.get_shader_program(&options).map_err(error)?;
        let q = cache.get_shader_program(&options).map_err(error)?;
        assert!(Rc::ptr_eq(&p, &q));
        let mut command = DrawCommand::new(&va, &p);
        command.set_dirty(false);
        assert!(!command.dirty());
        command.metadata.cull = false;
        assert!(command.dirty());
        let cloned = command.shallow_clone();
        assert!(cloned.dirty());
        assert!(!cloned.metadata.cull);
        ClearCommand::all().execute(&ctx, &PassState::default()).map_err(error)?;
        command.execute(&ctx, &PassState::default()).map_err(error)?;
        let pixels = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        let mut info = p.vertex_attributes().to_vec();
        info.sort_by(|a, b| a.name.cmp(&b.name));
        let result = json!({"layouts":layouts,"indices":va.index_buffer().unwrap().get_buffer_data(0,12).map_err(error)?,"pixels":pixels,"attributes":info,"programs":cache.number_of_shaders()});
        let derived = cache.create_derived_shader_program(&p, "derived", &options).map_err(error)?;
        assert!(Rc::ptr_eq(&derived, &cache.get_derived_shader_program(&p, "derived").unwrap()));
        let replacement = cache.replace_derived_shader_program(&p, "derived", &options).map_err(error)?;
        assert!(derived.is_destroyed());
        drop(cloned);
        drop(command);
        drop(p);
        drop(q);
        cache.destroy_released_shader_programs().map_err(error)?;
        assert!(replacement.is_destroyed());
        assert_eq!(cache.number_of_shaders(), 0);
        ctx.end_frame().map_err(error)?;
        Ok(result.to_string())
    }
    #[wasm_bindgen]
    pub fn renderer_uniform_arrays(
        gl: JsValue,
        vertex: &str,
        fragment: &str,
        specifications: &str,
    ) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let p = ShaderProgram::new(&ctx, vertex, fragment, &[]).map_err(error)?;
        let specs: Vec<serde_json::Value> = serde_json::from_str(specifications).map_err(error)?;
        let mut result = serde_json::Map::new();
        for s in specs {
            let name = s["name"].as_str().unwrap();
            let components = s["components"].as_u64().unwrap_or(1) as u32;
            let v = s["values"].as_array().unwrap();
            let value = match s["kind"].as_str().unwrap() {
                "float" => UniformValue::FloatArray {
                    components,
                    values: v.iter().map(|v| v.as_f64().unwrap() as f32).collect(),
                },
                "int" | "bool" => UniformValue::IntArray {
                    components,
                    values: v.iter().map(|v| v.as_i64().unwrap() as i32).collect(),
                },
                "uint" => UniformValue::UIntArray {
                    components,
                    values: v.iter().map(|v| v.as_u64().unwrap() as u32).collect(),
                },
                "matrix" => UniformValue::MatrixArray {
                    columns: components,
                    rows: components,
                    values: v.iter().map(|v| v.as_f64().unwrap() as f32).collect(),
                },
                _ => unreachable!(),
            };
            p.set_uniform(name, &value).map_err(error)?;
            let mut entries = vec![];
            for i in 0..2 {
                let value = p.get_uniform(&format!("{name}[{i}]")).map_err(error)?;
                entries.push(match value {
                    GlValue::Numbers(v) => json!(v),
                    GlValue::Number(v) => json!(v),
                    GlValue::Bool(v) => json!(v),
                    GlValue::Bools(v) => json!(v),
                    _ => panic!("Unexpected uniform value"),
                });
            }
            result.insert(name.into(), json!(entries));
        }
        Ok(serde_json::Value::Object(result).to_string())
    }
    #[wasm_bindgen]
    pub fn renderer_limits(gl: JsValue) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        Ok(serde_json::to_string(&ctx.limits().map_err(error)?).unwrap())
    }
    #[wasm_bindgen]
    pub fn renderer_shader_errors(gl: JsValue) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let good = "#version 300 es\nvoid main(){gl_Position=vec4(0.);}";
        let compile = ShaderProgram::new(
            &ctx,
            "#version 300 es\nBROKEN SHADER",
            "#version 300 es\nprecision highp float;out vec4 c;void main(){c=vec4(1.);}",
            &[],
        )
        .err()
        .unwrap()
        .message;
        let link=ShaderProgram::new(&ctx,"#version 300 es\nout vec2 varyingValue; void main(){gl_Position=vec4(0.);varyingValue=vec2(1.);}","#version 300 es\nprecision highp float;in vec3 varyingValue;out vec4 c;void main(){c=vec4(varyingValue,1.);}",&[]).err().unwrap().message;
        let p = ShaderProgram::new(
            &ctx,
            good,
            "#version 300 es\nprecision highp float;out vec4 c;void main(){c=vec4(1.);}",
            &[],
        )
        .map_err(error)?;
        p.set_uniform("optimizedAway", &UniformValue::Float(vec![1.])).map_err(error)?;
        p.destroy().map_err(error)?;
        assert!(p.is_destroyed());
        assert!(p.bind().is_err());
        Ok(json!([!compile.is_empty(), !link.is_empty()]).to_string())
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
