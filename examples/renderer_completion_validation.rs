//! Additional browser checks for the final Renderer modules.
#[cfg(target_arch = "wasm32")]
mod browser {
    use cesium_rs::{
        core::*,
        renderer::{browser::BrowserGl, *},
    };
    use serde_json::{json, Value};
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    fn error(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }
    #[wasm_bindgen]
    pub fn textures(gl: JsValue, config: &str, image: Option<JsValue>) -> Result<String, JsValue> {
        let c: Value = serde_json::from_str(config).map_err(error)?;
        let cube = c["kind"] == "cube";
        let mip = c["mip"].as_bool().unwrap_or(false);
        let width = if mip { 4 } else { 2 };
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut options = TextureOptions::rgba(width, width);
        options.flip_y = c["flip"].as_bool().unwrap_or(true);
        options.sampler = Sampler::NEAREST;
        if c["packed"].as_bool().unwrap_or(false) {
            options.pixel_datatype = PixelDatatype::UnsignedShort4_4_4_4;
        }
        let mut metadata;
        let mut faces = vec![];
        let binding = if cube {
            let source = CubeFace::ALL.map(|face| {
                if c["dom"].as_bool().unwrap_or(false) {
                    TextureSource::Image { width, height: width, image: image.clone().unwrap() }
                } else {
                    let i = face.index() as u8;
                    let bytes = (0..width * width)
                        .flat_map(|p| {
                            [16 + i * 32, 255 - i * 16 - (p / width) as u8 * 8, i * 20 + (p % width) as u8 * 8, 255]
                        })
                        .collect();
                    TextureSource::Pixels { width, height: width, bytes }
                }
            });
            let map = Rc::new(
                CubeMap::new(&ctx, options, if c["blank"].as_bool().unwrap_or(false) { None } else { Some(source) })
                    .map_err(error)?,
            );
            if c["patch"].as_bool().unwrap_or(false) {
                map.face(CubeFace::PositiveX)
                    .copy_from(
                        &TextureSource::Pixels { width: 1, height: 1, bytes: vec![255, 255, 255, 255] },
                        1,
                        1,
                        false,
                    )
                    .map_err(error)?;
            }
            if c["copy"].as_bool().unwrap_or(false) {
                let t = Rc::new(
                    Texture::new(
                        &ctx,
                        TextureOptions::rgba(width, width),
                        Some(&[0u8, 255, 255, 255].repeat((width * width) as usize)),
                    )
                    .map_err(error)?,
                );
                map.copy_face(t, CubeFace::NegativeZ, 0).map_err(error)?;
            }
            if mip {
                let levels: Vec<_> = [(2, [0u8, 255, 0, 255]), (1, [0, 0, 255, 255])]
                    .into_iter()
                    .map(|(size, color)| {
                        std::array::from_fn(|_| TextureSource::Pixels {
                            width: size,
                            height: size,
                            bytes: color.repeat((size * size) as usize),
                        })
                    })
                    .collect();
                map.load_mipmaps(&levels, false).map_err(error)?;
            }
            if c["generate"].as_bool().unwrap_or(false) {
                map.generate_mipmap(MipmapHint::DontCare).map_err(error)?;
            }
            if c["fbCopy"].as_bool().unwrap_or(false) || c["fbMip"].as_bool().unwrap_or(false) {
                ClearCommand { color: Some([0., 1., 1., 1.]), ..Default::default() }
                    .execute(&ctx, &PassState::default())
                    .map_err(error)?;
                if c["fbCopy"].as_bool().unwrap_or(false) {
                    map.face(CubeFace::PositiveX)
                        .copy_from_framebuffer(FramebufferCopy {
                            source: TextureRegion { x: 0, y: 0, width, height: width },
                            destination: [0, 0],
                            framebuffer: None,
                        })
                        .map_err(error)?;
                } else {
                    for face in CubeFace::ALL {
                        map.face(face)
                            .copy_mipmap_from_framebuffer(TextureRegion { x: 0, y: 0, width: 2, height: 2 }, 1, None)
                            .map_err(error)?;
                    }
                }
            }
            for face in CubeFace::ALL {
                let f = Framebuffer::new(
                    &ctx,
                    FramebufferOptions {
                        colors: vec![Attachment::CubeFace(map.face(face).clone())],
                        destroy_attachments: false,
                        ..Default::default()
                    },
                )
                .map_err(error)?;
                faces.push(
                    ctx.read_pixels_from(ReadState {
                        region: TextureRegion { x: 0, y: 0, width, height: width },
                        framebuffer: Some(&f),
                    })
                    .map_err(error)?
                    .bytes,
                );
            }
            metadata = json!({"dimensions":[map.width(),map.height()],"bytes":map.size_in_bytes(),"flip":map.flip_y(),"premultiply":map.pre_multiply_alpha()});
            TextureBinding::CubeMap(map)
        } else {
            let base: Vec<u8> = if c["packed"].as_bool().unwrap_or(false) {
                [0xf00fu16, 0x0f0f, 0x00ff, 0xffff, 0xff0f, 0xf0ff, 0x0fff, 0x888f]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect()
            } else if mip {
                [255u8, 0, 0, 255].repeat(64)
            } else {
                (0..8u8).flat_map(|i| [20 + i * 20, 40 + i * 10, 200 - i * 10, 255]).collect()
            };
            let levels = if mip { vec![[0u8, 255, 0, 255].repeat(8), vec![0, 0, 255, 255]] } else { vec![] };
            let map = Rc::new(Texture3D::with_mipmaps(&ctx, options, width, Some(&base), &levels).map_err(error)?);
            if c["patch"].as_bool().unwrap_or(false) {
                map.copy_from(TextureVolume { x: 1, y: 1, z: 1, width: 1, height: 1, depth: 1 }, &[255; 4])
                    .map_err(error)?;
            }
            if c["generate"].as_bool().unwrap_or(false) {
                map.generate_mipmap(MipmapHint::DontCare).map_err(error)?;
            }
            metadata = json!({"dimensions":map.dimensions(),"bytes":map.size_in_bytes(),"flip":map.flip_y(),"premultiply":map.pre_multiply_alpha()});
            TextureBinding::Volume(map)
        };
        let mut cache = ShaderCache::new(&ctx);
        let kind = if cube { "samplerCube" } else { "sampler3D" };
        let program=cache.get_shader_program(&ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(format!("uniform {kind} image;uniform vec3 coordinate;uniform float lod;void main(){{out_FragColor=textureLod(image,coordinate,lod);}}")))).map_err(error)?;
        let quad = ViewportQuad::new(&ctx).map_err(error)?;
        let coords: Vec<[f32; 3]> = if cube {
            CubeFace::ALL
                .map(|f| {
                    let v = f.direction();
                    [v.x as f32, v.y as f32, v.z as f32]
                })
                .to_vec()
        } else {
            (0..8)
                .map(|i| {
                    [
                        if i & 1 == 0 { 0.25 } else { 0.75 },
                        if i & 2 == 0 { 0.25 } else { 0.75 },
                        if i & 4 == 0 { 0.25 } else { 0.75 },
                    ]
                })
                .collect()
        };
        let mut pixels = vec![];
        for coord in coords {
            let mut command = quad.create_command(&program);
            command.samplers.push(("image".into(), binding.sampled()));
            command.uniforms = vec![
                ("coordinate".into(), UniformValue::Float(coord.to_vec())),
                ("lod".into(), UniformValue::Float(vec![c["lod"].as_f64().unwrap_or(0.) as f32])),
            ];
            command.execute(&ctx, &PassState::default()).map_err(error)?;
            pixels.push(ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 1, height: 1 }).map_err(error)?);
        }
        metadata["faces"] = json!(faces);
        metadata["pixels"] = json!(pixels);
        Ok(metadata.to_string())
    }
    #[wasm_bindgen]
    pub fn uniform_frame(config: &str) -> Result<String, JsValue> {
        let c: Value = serde_json::from_str(config).map_err(error)?;
        let mode = c["mode"].as_u64().unwrap_or(3) as u32;
        let ellipsoid = if c["sphere"].as_bool().unwrap_or(false) { Ellipsoid::UNIT_SPHERE } else { Ellipsoid::WGS84 };
        let cartographic = Cartographic::new(0.4, 0.2, 1000.);
        let position = if mode == 3 {
            ellipsoid.cartographic_to_cartesian(&cartographic)
        } else {
            Cartesian3::new(100000., 200000., 300000.)
        };
        let direction = if mode == 3 { position.negate().normalize() } else { Cartesian3::new(-1., 0., 0.) };
        let right = direction.cross(Cartesian3::UNIT_Z).normalize();
        let up = right.cross(direction);
        let view = Matrix4::compute_view(position, direction, up, right);
        let mut perspective = PerspectiveFrustum::new(1.1, 4. / 3.);
        perspective.near = 2.;
        perspective.far = 12345.;
        perspective.x_offset = 0.2;
        perspective.y_offset = -0.1;
        let mut ortho = OrthographicFrustum::new(100., 4. / 3.);
        ortho.near = 2.;
        ortho.far = 12345.;
        let frustum = match c["frustum"].as_str().unwrap_or("perspective") {
            "ortho" => UniformFrustum::Orthographic(ortho),
            "offcenter" => {
                UniformFrustum::PerspectiveOffCenter(PerspectiveOffCenterFrustum::new(-1., 2., 3., -2., 2., 12345.))
            }
            "orthoOffcenter" => UniformFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
                -50., 50., 40., -40., 2., 12345.,
            )),
            _ => UniformFrustum::Perspective(perspective),
        };
        let camera = UniformCamera {
            view,
            inverse_view: view.inverse_transformation(),
            position,
            direction,
            right,
            up,
            cartographic: if c["noCartographic"].as_bool().unwrap_or(false) { None } else { Some(cartographic) },
            frustum,
        };
        let date = JulianDate::from_iso8601(c["date"].as_str().unwrap_or("2025-01-02T03:04:05Z")).map_err(error)?;
        let mut frame = UniformFrame::new(camera, date);
        frame.mode = mode;
        frame.map_projection = if c["mercator"].as_bool().unwrap_or(false) {
            Rc::new(WebMercatorProjection::new(ellipsoid))
        } else {
            Rc::new(GeographicProjection::new(ellipsoid))
        };
        frame.frame_number = 37.;
        frame.morph_time = 0.75;
        frame.pixel_ratio = 2.;
        frame.split_position = 0.3;
        frame.maximum_screen_space_error = 8.;
        frame.fog_density = 0.001;
        frame.fog_visual_density_scalar = 2.;
        frame.fog_minimum_brightness = 0.1;
        frame.atmosphere = Some(UniformAtmosphere {
            hsb_shift: Cartesian3::new(0.1, 0.2, 0.3),
            light_intensity: 4.,
            rayleigh_coefficient: Cartesian3::new(0.001, 0.002, 0.003),
            rayleigh_scale_height: 10000.,
            mie_coefficient: Cartesian3::new(0.004, 0.005, 0.006),
            mie_scale_height: 3000.,
            mie_anisotropy: 0.8,
            dynamic_lighting: 2.,
        });
        frame.background_color = Color::new(0.1, 0.2, 0.3, 0.4);
        frame.invert_classification_color = Color::new(0.4, 0.3, 0.2, 0.1);
        frame.gamma = Some(2.2);
        frame.minimum_disable_depth_test_distance =
            if c["infiniteDistance"].as_bool().unwrap_or(false) { f64::INFINITY } else { 100. };
        frame.specular_environment_maps_maximum_lod = Some(5.);
        frame.spherical_harmonic_coefficients =
            (0..9).map(|i| Cartesian3::new(i as f64 * 0.1, i as f64 * 0.2, i as f64 * 0.3)).collect();
        if c["light"].as_bool().unwrap_or(false) {
            frame.light = UniformLight {
                direction: Some(Cartesian3::new(1., 2., 3.)),
                color: Color::new(0.3, 0.6, 0.9, 1.),
                intensity: 4.,
            };
        }
        let mut state = UniformState::default();
        state.viewport = Viewport { x: 7, y: 9, width: 640, height: 480 };
        state.model = Matrix4([2., 0., 0., 0., 0., 3., 0., 0., 0., 0., 4., 0., 12., 34., 56., 1.]);
        state.update(&frame, 800).map_err(error)?;
        state.update_pass(Pass::Opaque);
        if c["subfrustum"].as_bool().unwrap_or(false) {
            state.update_frustum_snapshot(&UniformFrustum::PerspectiveOffCenter(PerspectiveOffCenterFrustum::new(
                -1., 1., 1., -1., 3., 100.,
            )));
        }
        if c["moveCamera"].as_bool().unwrap_or(false) {
            let mut camera = frame.camera.clone();
            camera.position = camera.position.add(Cartesian3::new(10., 20., 30.));
            camera.cartographic = None;
            camera.view = Matrix4::compute_view(camera.position, camera.direction, camera.up, camera.right);
            camera.inverse_view = camera.view.inverse_transformation();
            state.update_camera_snapshot(&camera, &ellipsoid).unwrap();
        }

        let mut result = serde_json::Map::new();
        for (name, uniform) in automatic_uniforms() {
            if matches!(uniform.datatype, 35678 | 35680) {
                continue;
            }
            if let Some(value) = state.get(name).map_err(error)? {
                let values = match value {
                    UniformValue::Float(v)
                    | UniformValue::FloatArray { values: v, .. }
                    | UniformValue::Matrix { values: v, .. }
                    | UniformValue::MatrixArray { values: v, .. } => v,
                    _ => return Err(error("Unexpected numeric uniform")),
                };
                result.insert(name.clone(), json!(values));
            }
        }
        Ok(Value::Object(result).to_string())
    }

    #[wasm_bindgen]
    pub async fn atlas(gl: JsValue) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut atlas =
            TextureAtlas::new(TextureAtlasOptions { initial_size: [4, 4], ..Default::default() }).map_err(error)?;
        let image =
            |w, h, color: &[u8]| TextureSource::Pixels { width: w, height: h, bytes: color.repeat((w * h) as usize) };
        let a = atlas.add_image("a", image(2, 2, &[255, 0, 0, 255])).map_err(error)?;
        let b = atlas.add_image("b", image(1, 3, &[0, 255, 0, 255])).map_err(error)?;
        atlas.update(&ctx).map_err(error)?;
        let indices = vec![a.await.map_err(error)?, b.await.map_err(error)?];
        let c = atlas.add_image("c", image(5, 3, &[0, 0, 255, 255])).map_err(error)?;
        let sub = atlas.add_image_sub_region("a", BoundingRectangle::new(1., 0., 1., 2.)).map_err(error)?;
        atlas.update(&ctx).map_err(error)?;
        let mut indices = indices;
        indices.push(c.await.map_err(error)?);
        indices.push(sub.await.map_err(error)?);
        let coords = (0..atlas.number_of_images())
            .map(|i| {
                let r = atlas.compute_texture_coordinates(i).map_err(error)?;
                Ok([r.x, r.y, r.width, r.height])
            })
            .collect::<Result<Vec<_>, JsValue>>()?;
        let t = atlas.texture().unwrap();
        let f = Framebuffer::new(
            &ctx,
            FramebufferOptions {
                colors: vec![Attachment::Texture(t.clone())],
                destroy_attachments: false,
                ..Default::default()
            },
        )
        .map_err(error)?;
        let pixels = ctx
            .read_pixels_from(ReadState {
                framebuffer: Some(&f),
                region: TextureRegion { x: 0, y: 0, width: t.width(), height: t.height() },
            })
            .map_err(error)?
            .bytes;
        Ok(json!({"indices":indices,"coords":coords,"size":t.dimensions(),"bytes":atlas.size_in_bytes(),"pixels":pixels}).to_string())
    }
    #[wasm_bindgen]
    pub fn facade(gl: JsValue, config: &str) -> Result<String, JsValue> {
        let c: Value = serde_json::from_str(config).map_err(error)?;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let size = c["size"].as_u64().unwrap_or(3) as usize;
        let mut color = FacadeAttribute::new(1, 4);
        color.datatype = ComponentDatatype::UnsignedByte;
        color.normalize = true;
        let mut short = FacadeAttribute::new(2, 2);
        short.datatype = ComponentDatatype::UnsignedShort;
        let mut dynamic = FacadeAttribute::new(3, 1);
        dynamic.usage = BufferUsage::DynamicDraw;
        let mut facade =
            VertexArrayFacade::new(&ctx, vec![FacadeAttribute::new(0, 2), color, short, dynamic], size, false)
                .map_err(error)?;
        for i in 0..size {
            facade.write(0, i, &[if i == 1 { 3. } else { -1. }, if i == 2 { 3. } else { -1. }]).map_err(error)?;
            facade.write(1, i, &[255., 128., 64., 255.]).map_err(error)?;
            facade.write(2, i, &[i as f64, 1000. + i as f64]).map_err(error)?;
            facade.write(3, i, &[i as f64 + 0.5]).map_err(error)?;
        }
        let indices = if c["indexed"].as_bool().unwrap_or(false) {
            Some(
                Buffer::new(
                    &ctx,
                    BufferKind::Index(IndexDatatype::UnsignedShort),
                    BufferUsage::StaticDraw,
                    BufferData::Bytes(&[0, 0, 1, 0, 2, 0]),
                )
                .map_err(error)?,
            )
        } else {
            None
        };
        facade.commit(indices.clone()).map_err(error)?;
        if c["subcommit"].as_bool().unwrap_or(false) {
            facade.write(1, 1, &[0., 255., 255., 255.]).map_err(error)?;
            facade.sub_commit(1, 1).map_err(error)?;
            facade.end_sub_commits();
            facade.commit(indices.clone()).map_err(error)?;
        }
        if c["resize"].as_bool().unwrap_or(false) {
            facade.resize(size * 2).map_err(error)?;
            facade.write(0, size, &[0., 0.]).map_err(error)?;
            facade.commit(indices).map_err(error)?;
        }
        let arrays = facade
            .vertex_arrays()
            .unwrap()
            .iter()
            .map(|va| {
                let attrs = va
                    .vertex_array
                    .attributes()
                    .iter()
                    .map(|a| {
                        json!([
                            a.index,
                            a.components,
                            a.datatype as u32,
                            a.normalize,
                            a.offset,
                            a.stride,
                            a.instance_divisor
                        ])
                    })
                    .collect::<Vec<_>>();
                json!({"count":va.indices_count,"attributes":attrs})
            })
            .collect::<Vec<_>>();
        let mut buffers = vec![];
        let va = &facade.vertex_arrays().unwrap()[0].vertex_array;
        for a in va.attributes() {
            if let AttributeSource::Buffer(b) = &a.source {
                if a.offset == 0 && size < 100 {
                    buffers.push(b.get_buffer_data(0, b.size_in_bytes()).map_err(error)?);
                }
            }
        }
        let mut pixels = vec![];
        if size < 100 {
            let program=ctx.with_shader_cache(|cache|cache.get_shader_program(&ShaderProgramOptions{vertex_shader_source:ShaderSource::new("in vec2 position;in vec4 color;out vec4 v;void main(){gl_Position=vec4(position,0.,1.);v=color;}"),fragment_shader_source:ShaderSource::new("in vec4 v;void main(){out_FragColor=v;}"),attribute_locations:std::collections::BTreeMap::from([("position".into(),0),("color".into(),1)])})).map_err(error)?;
            let mut draw = DrawCommand::new(va, &program);
            draw.count = Some(3);
            draw.execute(&ctx, &PassState::default()).map_err(error)?;
            pixels = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 1, height: 1 }).map_err(error)?;
        }
        Ok(json!({"arrays":arrays,"buffers":buffers,"pixels":pixels}).to_string())
    }
    #[wasm_bindgen]
    pub fn context_api(gl: JsValue, webgl1: bool) -> Result<String, JsValue> {
        let ctx = if webgl1 {
            Context::new_webgl1(BrowserGl::new_webgl1(gl).map_err(error)?)
        } else {
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?)
        };
        let mut colors = vec![];
        for t in [
            ctx.default_texture().map_err(error)?,
            ctx.default_emissive_texture().map_err(error)?,
            ctx.default_normal_texture().map_err(error)?,
        ] {
            let f = Framebuffer::new(
                &ctx,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(t)],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?;
            colors.push(
                ctx.read_pixels_from(ReadState {
                    framebuffer: Some(&f),
                    region: TextureRegion { x: 0, y: 0, width: 1, height: 1 },
                })
                .map_err(error)?
                .bytes,
            );
        }
        let a = ctx.create_pick_id(Rc::new(17u32)).map_err(error)?;
        let b = ctx.create_pick_id(Rc::new(42u32)).map_err(error)?;
        let picks = vec![a.color.to_bytes(), b.color.to_bytes()];
        a.destroy();
        let found = ctx.pick_object::<u32>(b.key).map(|v| *v);
        ctx.validate_shader_program.set(true);
        ctx.validate_framebuffer.set(true);
        ctx.set_throw_on_webgl_error(true);
        ctx.uniform_state().set("czm_backgroundColor", UniformValue::Float(vec![0.25, 0.5, 0.75, 1.]));
        let command = ctx
            .create_viewport_quad_command(
                ShaderSource::new("void main(){out_FragColor=czm_backgroundColor;}"),
                ViewportCommandOptions::default(),
            )
            .map_err(error)?;
        command.execute(&ctx, &PassState::default()).map_err(error)?;
        let pixels = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 1, height: 1 }).map_err(error)?;
        let target = Rc::new(Texture::new(&ctx, TextureOptions::rgba(2, 2), None).map_err(error)?);
        let framebuffer = Rc::new(
            Framebuffer::new(
                &ctx,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(target)],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?,
        );
        let offscreen = ctx
            .create_viewport_quad_command(
                ShaderSource::new("void main(){out_FragColor=czm_backgroundColor;}"),
                ViewportCommandOptions {
                    framebuffer: Some(framebuffer.clone()),
                    render_state: RenderState {
                        viewport: Some(Viewport { x: 0, y: 0, width: 2, height: 2 }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(error)?;
        offscreen.execute(&ctx, &PassState::default()).map_err(error)?;
        let offscreen_pixels = ctx
            .read_pixels_from(ReadState {
                framebuffer: Some(&framebuffer),
                region: TextureRegion { x: 0, y: 0, width: 1, height: 1 },
            })
            .map_err(error)?
            .bytes;
        if offscreen_pixels != pixels {
            return Err(error("Debug validation changed target bindings"));
        }
        let same = Rc::ptr_eq(&ctx.get_viewport_quad_vertex_array().map_err(error)?, &command.vertex_array);
        for _ in 0..2 {
            VertexArray::new(&ctx, vec![VertexAttribute::constant(0, vec![0.])], None)
                .map_err(error)?
                .destroy()
                .map_err(error)?;
        }
        ctx.end_frame().map_err(error)?;
        let limits = ctx.limits().map_err(error)?;
        let capabilities = ctx.capabilities().map_err(error)?;
        Ok(json!({"colors":colors,"picks":picks,"found":found,"pixels":pixels,"sameQuad":same,"webgl2":capabilities.webgl2,"max3D":limits.maximum_3d_texture_size,"stencilBits":ctx.stencil_bits().map_err(error)?,"antialias":ctx.antialias().map_err(error)?}).to_string())
    }
    #[wasm_bindgen]
    pub fn shared(gl: JsValue, surfaces: js_sys::Array, auto_destroy: bool) -> Result<String, JsValue> {
        use cesium_rs::renderer::shared_context::BrowserSharedSurfaceHost;
        let shared = SharedContext::new(
            Context::new_webgl2(BrowserGl::new(gl).map_err(error)?),
            BrowserSharedSurfaceHost,
            auto_destroy,
        );
        let a = shared.create_scene_context(surfaces.get(0)).map_err(error)?;
        let b = shared.create_scene_context(surfaces.get(1)).map_err(error)?;
        let duplicate = shared.create_scene_context(surfaces.get(0)).is_err();
        a.begin_frame().map_err(error)?;
        ClearCommand { color: Some([1., 0., 0., 1.]), ..Default::default() }
            .execute(a.context().map_err(error)?, &PassState::default())
            .map_err(error)?;
        a.end_frame().map_err(error)?;
        b.begin_frame().map_err(error)?;
        ClearCommand { color: Some([0., 1., 0., 1.]), ..Default::default() }
            .execute(b.context().map_err(error)?, &PassState::default())
            .map_err(error)?;
        // A blue bottom strip checks the top-down presentation crop.
        ClearCommand {
            color: Some([0., 0., 1., 1.]),
            render_state: RenderState {
                scissor_test: cesium_rs::renderer::render_state::ScissorTest {
                    enabled: true,
                    rectangle: Viewport { x: 0, y: 0, width: 4, height: 2 },
                },
                ..Default::default()
            },
            ..Default::default()
        }
        .execute(b.context().map_err(error)?, &PassState::default())
        .map_err(error)?;
        b.end_frame().map_err(error)?;
        a.destroy().map_err(error)?;
        let after_first = shared.is_destroyed();
        b.destroy().map_err(error)?;
        let result = json!({"duplicate":duplicate,"afterFirst":after_first,"afterLast":shared.is_destroyed(),"active":shared.active_scenes()});
        if !auto_destroy {
            shared.destroy().map_err(error)?;
        }
        Ok(result.to_string())
    }

    #[wasm_bindgen]
    pub fn factory(gl: JsValue, config: &str) -> Result<String, JsValue> {
        let c: Value = serde_json::from_str(config).map_err(error)?;
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let source = c["shader"].as_str().unwrap();
        let program = ctx
            .with_shader_cache(|cache| {
                cache.get_shader_program(&ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(source)))
            })
            .map_err(error)?;
        program.bind().map_err(error)?;
        let datatype = c["datatype"].as_u64().unwrap() as u32;
        let components = c["components"].as_u64().unwrap() as u32;
        let array = c["array"].as_bool().unwrap_or(false);
        let matrix = c["matrix"].as_bool().unwrap_or(false);
        let signed = c["signed"].as_bool().unwrap_or(false);
        let values: Vec<f32> = c["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        let value = if matrix {
            if array {
                UniformValue::MatrixArray { columns: components, rows: components, values }
            } else {
                UniformValue::Matrix { dimension: components, values }
            }
        } else if signed {
            let values = values.iter().map(|v| *v as i32).collect();
            if array {
                UniformValue::IntArray { components, values }
            } else {
                UniformValue::Int(values)
            }
        } else if array {
            UniformValue::FloatArray { components, values }
        } else {
            UniformValue::Float(values)
        };
        let binding = if array {
            create_uniform_array(&program, "value", datatype, 2)
        } else {
            create_uniform(&program, "value", datatype)
        }
        .map_err(error)?;
        binding.set(&value).map_err(error)?;
        binding.set(&value).map_err(error)?;
        let va = ctx.get_viewport_quad_vertex_array().map_err(error)?;
        DrawCommand::new(&va, &program).execute(&ctx, &PassState::default()).map_err(error)?;
        Ok(json!(ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 1, height: 1 }).map_err(error)?).to_string())
    }
    #[wasm_bindgen]
    pub fn automatic_textures(gl: JsValue, default_environment: bool) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let camera = UniformCamera {
            view: Matrix4::IDENTITY,
            inverse_view: Matrix4::IDENTITY,
            position: Cartesian3::new(6379137., 0., 0.),
            direction: Cartesian3::new(-1., 0., 0.),
            up: Cartesian3::UNIT_Z,
            right: Cartesian3::UNIT_Y,
            cartographic: Some(Cartographic::new(0., 0., 1000.)),
            frustum: UniformFrustum::Perspective(PerspectiveFrustum::new(1., 1.)),
        };
        let frame = UniformFrame::new(camera, JulianDate::from_iso8601("2025-01-01T00:00:00Z").map_err(error)?);
        let mut inputs = FrameTextures::default();
        for (name, color) in [
            ("czm_globeDepthTexture", [255, 0, 0, 255]),
            ("czm_edgeIdTexture", [0, 255, 0, 255]),
            ("czm_edgeColorTexture", [0, 0, 255, 255]),
            ("czm_edgeDepthTexture", [255, 255, 0, 255]),
            ("czm_brdfLut", [0, 255, 255, 255]),
        ] {
            inputs.textures.insert(
                name.into(),
                TextureBinding::Texture(Rc::new(
                    Texture::new(&ctx, TextureOptions::rgba(1, 1), Some(&color)).map_err(error)?,
                )),
            );
        }
        for (name, color) in
            [("czm_environmentMap", [255, 0, 255, 255]), ("czm_specularEnvironmentMaps", [128, 128, 128, 255])]
        {
            if default_environment && name == "czm_environmentMap" {
                continue;
            }
            inputs.textures.insert(
                name.into(),
                TextureBinding::CubeMap(Rc::new(
                    CubeMap::new(
                        &ctx,
                        TextureOptions::rgba(1, 1),
                        Some(std::array::from_fn(|_| TextureSource::Pixels {
                            width: 1,
                            height: 1,
                            bytes: color.to_vec(),
                        })),
                    )
                    .map_err(error)?,
                )),
            );
        }
        ctx.update_frame(&frame, &inputs).map_err(error)?;
        let mut command=ctx.create_viewport_quad_command(ShaderSource::new("uniform sampler2D image;void main(){vec2 p=vec2(0.5);vec3 d=vec3(1.,0.,0.);out_FragColor=(texture(image,p)+texture(czm_globeDepthTexture,p)+texture(czm_edgeIdTexture,p)+texture(czm_edgeColorTexture,p)+texture(czm_edgeDepthTexture,p)+texture(czm_brdfLut,p)+texture(czm_environmentMap,d)+texture(czm_specularEnvironmentMaps,d))/8.;}"),ViewportCommandOptions::default()).map_err(error)?;
        command.textures.push(("image".into(), TextureBinding::Texture(ctx.default_texture().map_err(error)?)));
        command.execute(&ctx, &PassState::default()).map_err(error)?;
        let result = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 1, height: 1 }).map_err(error)?;
        ctx.end_frame().map_err(error)?;
        Ok(json!(result).to_string())
    }

    #[wasm_bindgen]
    pub async fn cube_loader(gl: JsValue, url: &str, skip: bool) -> Result<String, JsValue> {
        use cesium_rs::core::browser_host::{BrowserImageHost, BrowserTransport};
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let client = ResourceClient::new(Rc::new(BrowserTransport::default()));
        let resources = std::array::from_fn(|_| Resource::new(url).unwrap());
        let cube =
            load_cube_map_resources(&ctx, resources, skip, &client, Rc::new(BrowserImageHost)).await.map_err(error)?;
        let mut pixels = vec![];
        for face in CubeFace::ALL {
            let framebuffer = Framebuffer::new(
                &ctx,
                FramebufferOptions {
                    colors: vec![Attachment::CubeFace(cube.face(face).clone())],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?;
            pixels.push(
                ctx.read_pixels_from(ReadState {
                    framebuffer: Some(&framebuffer),
                    region: TextureRegion { x: 0, y: 0, width: cube.width(), height: cube.height() },
                })
                .map_err(error)?
                .bytes,
            );
        }
        Ok(json!(pixels).to_string())
    }
}
#[allow(dead_code)]
fn main() {}
