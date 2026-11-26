//! Interactive full-feature live testbed for CesiumRS compiled to WebAssembly.
//! Exposes Core, Scene, Renderer, DataSources, Workers, Shaders, and Browser Host
//! capabilities to `test-site/` via `wasm-bindgen`.

#[cfg(target_arch = "wasm32")]
mod testbed {
    use cesium_rs::{
        camera::{look_at, pick_ray, ray_from_ndc, CameraFrame},
        core::{
            browser_host::{BrowserImageHost, BrowserTransport},
            clock::ClockOptions,
            math::{to_degrees, to_radians, PI, PI_OVER_TWO},
            simon1994_planetary_positions::{
                compute_moon_position_in_earth_inertial_frame, compute_sun_position_in_earth_inertial_frame,
            },
            *,
        },
        data_sources::*,
        geodesic::Geodesic,
        mesh::{build_geodesic_arc, build_globe, build_graticule, build_stars, build_tile_grid, suggested_tile_level},
        renderer::{browser::BrowserGl, ViewportQuad, *},
        scene::{
            auto_exposure::AutoExposure,
            brdf_lut_generator::BrdfLutGenerator,
            gltf_pipeline::BufferSource,
            model::{
                CustomShader, CustomShaderMode, CustomShaderOptions, CustomShaderTranslucencyMode, LightingModel,
                Model, ModelOptions,
            },
            post_process_stage_collection::PostProcessStageCollection,
            post_process_stage_library::PostProcessStageLibrary,
            tonemapper::Tonemapper,
            *,
        },
        shaders,
        sun::sun_direction_ecef,
        workers::{self, geometry::GeometryDescription},
    };
    use serde_json::{json, Value};
    use std::{
        cell::RefCell,
        collections::{BTreeMap, HashMap},
        f64::consts::FRAC_PI_3,
        rc::Rc,
    };
    use wasm_bindgen::prelude::*;

    fn err(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }

    fn f32_bytes(slice: &[f32]) -> &[u8] {
        unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), std::mem::size_of_val(slice)) }
    }

    fn u32_bytes(slice: &[u32]) -> &[u8] {
        unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), std::mem::size_of_val(slice)) }
    }

    fn mat4_uniform(values: Vec<f32>) -> UniformValue {
        UniformValue::Matrix { dimension: 4, values }
    }

    fn mat4_f32(m: &[f64; 16]) -> Vec<f32> {
        m.iter().map(|&v| v as f32).collect()
    }

    fn ellipsoid_by_name(name: &str) -> Ellipsoid {
        match name {
            "moon" => Ellipsoid::MOON,
            "mars" => Ellipsoid::MARS,
            "unit" => Ellipsoid::UNIT_SPHERE,
            _ => Ellipsoid::WGS84,
        }
    }

    fn create_line_va(context: &Context<BrowserGl>, positions: &[f32]) -> Result<VertexArray<BrowserGl>, RuntimeError> {
        let buf = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(f32_bytes(positions)))?;
        let mut attr = VertexAttribute::buffer(0, buf, 3);
        attr.stride = 12;
        VertexArray::new(context, vec![attr], None)
    }

    fn create_mesh_va(
        context: &Context<BrowserGl>,
        vertices: &[f32], // pos3 + norm3
        indices: &[u32],
    ) -> Result<VertexArray<BrowserGl>, RuntimeError> {
        let vbuf = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(f32_bytes(vertices)))?;
        let ibuf = Buffer::new(
            context,
            BufferKind::Index(IndexDatatype::UnsignedInt),
            BufferUsage::StaticDraw,
            BufferData::Bytes(u32_bytes(indices)),
        )?;
        let mut pos = VertexAttribute::buffer(0, vbuf.clone(), 3);
        pos.stride = 24;
        pos.offset = 0;
        let mut norm = VertexAttribute::buffer(1, vbuf, 3);
        norm.stride = 24;
        norm.offset = 12;
        VertexArray::new(context, vec![pos, norm], Some(ibuf))
    }

    fn extract_geometry_pos_norm_idx(geom: &Geometry) -> Option<(Vec<f32>, Vec<u32>, Vec<u32>)> {
        let pos_attr = geom.attributes.position.as_ref()?;
        let pos_f64: Vec<f64> = match &pos_attr.values {
            TypedArray::Float64(v) => v.clone(),
            TypedArray::Float32(v) => v.iter().map(|&x| f64::from(x)).collect(),
            _ => return None,
        };
        let n_verts = pos_f64.len() / 3;
        if n_verts == 0 {
            return None;
        }
        let norm_f32: Option<Vec<f32>> = geom.attributes.normal.as_ref().and_then(|na| match &na.values {
            TypedArray::Float32(v) => Some(v.clone()),
            TypedArray::Float64(v) => Some(v.iter().map(|&x| x as f32).collect()),
            _ => None,
        });
        let indices: Vec<u32> = match &geom.indices {
            Some(TypedArray::Uint32(v)) => v.clone(),
            Some(TypedArray::Uint16(v)) => v.iter().map(|&x| u32::from(x)).collect(),
            Some(TypedArray::Uint8(v)) => v.iter().map(|&x| u32::from(x)).collect(),
            None => (0..n_verts as u32).collect(),
            _ => return None,
        };
        let mut normals = if let Some(n) = norm_f32 {
            if n.len() >= n_verts * 3 {
                n
            } else {
                vec![0.0; n_verts * 3]
            }
        } else {
            vec![0.0; n_verts * 3]
        };
        if geom.attributes.normal.is_none() && geom.primitive_type == PrimitiveType::Triangles {
            for tri in indices.chunks_exact(3) {
                let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
                if i0 < n_verts && i1 < n_verts && i2 < n_verts {
                    let a = Cartesian3::new(pos_f64[i0 * 3], pos_f64[i0 * 3 + 1], pos_f64[i0 * 3 + 2]);
                    let b = Cartesian3::new(pos_f64[i1 * 3], pos_f64[i1 * 3 + 1], pos_f64[i1 * 3 + 2]);
                    let c = Cartesian3::new(pos_f64[i2 * 3], pos_f64[i2 * 3 + 1], pos_f64[i2 * 3 + 2]);
                    let n = (b - a).cross(c - a);
                    for &idx in &[i0, i1, i2] {
                        normals[idx * 3] += n.x as f32;
                        normals[idx * 3 + 1] += n.y as f32;
                        normals[idx * 3 + 2] += n.z as f32;
                    }
                }
            }
            for i in 0..n_verts {
                let x = normals[i * 3];
                let y = normals[i * 3 + 1];
                let z = normals[i * 3 + 2];
                let len = (x * x + y * y + z * z).sqrt().max(1e-6);
                normals[i * 3] = x / len;
                normals[i * 3 + 1] = y / len;
                normals[i * 3 + 2] = z / len;
            }
        }
        let mut interleaved = Vec::with_capacity(n_verts * 6);
        for i in 0..n_verts {
            interleaved.push(pos_f64[i * 3] as f32);
            interleaved.push(pos_f64[i * 3 + 1] as f32);
            interleaved.push(pos_f64[i * 3 + 2] as f32);
            interleaved.push(normals[i * 3]);
            interleaved.push(normals[i * 3 + 1]);
            interleaved.push(normals[i * 3 + 2]);
        }
        let wire_indices = if geom.primitive_type == PrimitiveType::Triangles {
            let wire_arr = wireframe_index_generator::create_wireframe_indices(
                PrimitiveType::Triangles,
                n_verts,
                Some(&indices),
            );
            match wire_arr {
                Some(TypedArray::Uint32(v)) => v,
                Some(TypedArray::Uint16(v)) => v.into_iter().map(u32::from).collect(),
                _ => Vec::new(),
            }
        } else {
            indices.clone()
        };
        Some((interleaved, indices, wire_indices))
    }

    // =========================================================================
    // 1. LIVE WEBGL2 GLOBE & SCENE VIEWPORT (Context<BrowserGl> + PostProcess)
    // =========================================================================

    const GLOBE_VS: &str = r#"#version 300 es
in vec3 a_position;
in vec3 a_normal;
in vec2 a_uv;
uniform mat4 u_viewProjection;
uniform float u_scale;
uniform int u_sceneMode;
uniform float u_bodyRadius;
out vec3 v_normal;
out vec3 v_position;
out vec2 v_uv;
void main() {
    vec3 p = a_position * u_scale;
    if (u_sceneMode == 1) {
        // 2D Equirectangular projection mapped onto a flat plane in world space
        float lon = (a_uv.x - 0.5) * 6.28318530718;
        float lat = (0.5 - a_uv.y) * 3.14159265359;
        p = vec3(lon * u_bodyRadius, lat * u_bodyRadius, (u_scale - 1.0) * u_bodyRadius);
        v_normal = vec3(0.0, 0.0, 1.0);
    } else if (u_sceneMode == 2) {
        // Columbus View (2.5D tilted slab)
        float lon = (a_uv.x - 0.5) * 6.28318530718;
        float lat = (0.5 - a_uv.y) * 3.14159265359;
        p = vec3(lon * u_bodyRadius, lat * u_bodyRadius, (u_scale - 1.0) * u_bodyRadius * 2.0);
        v_normal = vec3(0.0, 0.0, 1.0);
    } else {
        v_normal = a_normal;
    }
    v_position = p;
    v_uv = a_uv;
    gl_Position = u_viewProjection * vec4(p, 1.0);
}
"#;

    const GLOBE_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_texture;
uniform vec3 u_eye;
uniform vec3 u_sun;
uniform float u_sunLighting;
uniform int u_bodyKind;
uniform float u_clipEnabled;
uniform vec4 u_clipPlane;
in vec3 v_normal;
in vec3 v_position;
in vec2 v_uv;
out vec4 out_color;

float hash21(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}
float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash21(i);
    float b = hash21(i + vec2(1.0, 0.0));
    float c = hash21(i + vec2(0.0, 1.0));
    float d = hash21(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}
float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p *= 2.04;
        a *= 0.5;
    }
    return v;
}

void main() {
    if (u_clipEnabled > 0.5 && dot(v_position, u_clipPlane.xyz) + u_clipPlane.w < 0.0) {
        discard;
    }
    vec3 n = normalize(v_normal);
    vec3 toEye = normalize(u_eye - v_position);
    vec3 color;
    if (u_bodyKind == 0) {
        color = texture(u_texture, v_uv).rgb;
    } else if (u_bodyKind == 1) {
        // Moon procedural maria + craters
        float h = fbm(v_uv * vec2(18.0, 9.0));
        float c = fbm(v_uv * vec2(48.0, 24.0) + 3.1);
        color = mix(vec3(0.28, 0.29, 0.31), vec3(0.76, 0.75, 0.73), smoothstep(0.35, 0.72, h));
        color *= 0.82 + 0.36 * c;
    } else if (u_bodyKind == 2) {
        // Mars procedural rust + polar caps
        float h = fbm(v_uv * vec2(14.0, 7.0));
        float d = fbm(v_uv * vec2(36.0, 18.0) + 7.4);
        color = mix(vec3(0.45, 0.19, 0.11), vec3(0.84, 0.44, 0.24), h);
        color *= 0.85 + 0.3 * d;
        float polar = smoothstep(0.84, 0.94, abs(v_uv.y - 0.5) * 2.0);
        color = mix(color, vec3(0.92, 0.91, 0.88), polar);
    } else {
        // Unit sphere geodesic checker
        vec2 g = floor(v_uv * vec2(24.0, 12.0));
        float chk = mod(g.x + g.y, 2.0);
        color = mix(vec3(0.14, 0.24, 0.36), vec3(0.22, 0.38, 0.54), chk);
    }
    float facing = max(dot(n, toEye), 0.0);
    float head = 0.62 + 0.38 * facing;
    float sunDot = dot(n, u_sun);
    float day = smoothstep(-0.12, 0.22, sunDot);
    float lit = mix(0.10, 1.12, day) * (0.85 + 0.15 * facing);
    color *= mix(head, lit, u_sunLighting);
    if (u_bodyKind == 0 || u_bodyKind == 2) {
        vec3 atmoColor = u_bodyKind == 0 ? vec3(0.38, 0.65, 1.0) : vec3(0.88, 0.52, 0.32);
        float rim = pow(1.0 - facing, 3.0);
        color = mix(color, atmoColor, rim * 0.52);
    }
    out_color = vec4(color, 1.0);
}
"#;

    const HALO_FS: &str = r#"#version 300 es
precision highp float;
uniform vec3 u_eye;
uniform int u_bodyKind;
in vec3 v_normal;
in vec3 v_position;
in vec2 v_uv;
out vec4 out_color;
void main() {
    vec3 n = normalize(v_normal);
    vec3 toEye = normalize(u_eye - v_position);
    float edge = clamp(-dot(n, toEye) / 0.30, 0.0, 1.0);
    float glow = pow(edge, 2.2) * 0.82;
    vec3 tint = u_bodyKind == 2 ? vec3(0.88, 0.48, 0.28) : vec3(0.32, 0.60, 1.0);
    out_color = vec4(tint * glow, glow);
}
"#;

    const LINE_VS: &str = r#"#version 300 es
in vec3 a_position;
uniform mat4 u_viewProjection;
uniform float u_scale;
uniform float u_pointSize;
void main() {
    gl_Position = u_viewProjection * vec4(a_position * u_scale, 1.0);
    gl_PointSize = u_pointSize;
}
"#;

    const LINE_FS: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_color;
uniform float u_round;
out vec4 out_color;
void main() {
    if (u_round > 0.5) {
        vec2 d = gl_PointCoord - vec2(0.5);
        float r = length(d);
        if (r > 0.5) discard;
        float ring = smoothstep(0.32, 0.42, r);
        out_color = vec4(mix(u_color.rgb, vec3(0.03, 0.05, 0.08), ring), u_color.a);
        return;
    }
    out_color = u_color;
}
"#;

    const MESH_VS: &str = r#"#version 300 es
in vec3 a_position;
in vec3 a_normal;
uniform mat4 u_viewProjection;
uniform mat4 u_model;
out vec3 v_normal;
out vec3 v_worldPos;
void main() {
    vec4 wp = u_model * vec4(a_position, 1.0);
    v_worldPos = wp.xyz;
    v_normal = normalize(mat3(u_model) * a_normal);
    gl_Position = u_viewProjection * wp;
}
"#;

    const MESH_FS: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_color;
uniform vec3 u_sun;
uniform vec3 u_eye;
uniform float u_clipEnabled;
uniform vec4 u_clipPlane;
in vec3 v_normal;
in vec3 v_worldPos;
out vec4 out_color;
void main() {
    if (u_clipEnabled > 0.5 && dot(v_worldPos, u_clipPlane.xyz) + u_clipPlane.w < 0.0) {
        discard;
    }
    vec3 n = normalize(v_normal);
    vec3 v = normalize(u_eye - v_worldPos);
    vec3 l = normalize(u_sun);
    float diff = max(dot(n, l), 0.0) * 0.65 + max(dot(n, v), 0.0) * 0.35;
    vec3 h = normalize(l + v);
    float spec = pow(max(dot(n, h), 0.0), 32.0) * 0.35;
    vec3 col = u_color.rgb * (0.28 + 0.82 * diff) + vec3(spec);
    out_color = vec4(col, u_color.a);
}
"#;

    const STAR_VS: &str = r#"#version 300 es
in vec4 a_star;
uniform mat4 u_view;
uniform mat4 u_projection;
uniform float u_pixelRatio;
out float v_brightness;
void main() {
    vec3 dir = mat3(u_view[0].xyz, u_view[1].xyz, u_view[2].xyz) * a_star.xyz;
    gl_Position = u_projection * vec4(dir, 1.0);
    gl_Position.z = gl_Position.w * 0.99999;
    gl_PointSize = (1.1 + a_star.w * 1.8) * u_pixelRatio;
    v_brightness = a_star.w;
}
"#;

    const STAR_FS: &str = r#"#version 300 es
precision mediump float;
in float v_brightness;
out vec4 out_color;
void main() {
    float r = length(gl_PointCoord - vec2(0.5));
    if (r > 0.5) discard;
    out_color = vec4(vec3(0.84, 0.90, 1.0) * v_brightness, 1.0);
}
"#;

    struct OffscreenTarget {
        color: Rc<Texture<BrowserGl>>,
        depth: Rc<Texture<BrowserGl>>,
        framebuffer: Rc<Framebuffer<BrowserGl>>,
    }

    impl OffscreenTarget {
        fn new(context: &Context<BrowserGl>, width: u32, height: u32) -> Result<Self, RuntimeError> {
            let mut color_opts = TextureOptions::rgba(width, height);
            color_opts.flip_y = false;
            color_opts.sampler = Sampler {
                minification_filter: TextureMinificationFilter::Linear,
                magnification_filter: TextureMagnificationFilter::Linear,
                ..Default::default()
            };
            let color = Rc::new(Texture::new(context, color_opts, None)?);
            let mut depth_opts = TextureOptions::rgba(width, height);
            depth_opts.pixel_format = PixelFormat::DepthComponent;
            depth_opts.pixel_datatype = PixelDatatype::UnsignedInt;
            depth_opts.flip_y = false;
            depth_opts.sampler = Sampler::NEAREST;
            let depth = Rc::new(Texture::new(context, depth_opts, None)?);
            let framebuffer = Rc::new(Framebuffer::new(
                context,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(color.clone())],
                    depth: Some(Attachment::Texture(depth.clone())),
                    destroy_attachments: false,
                    ..Default::default()
                },
            )?);
            Ok(Self { color, depth, framebuffer })
        }
    }

    struct GeometrySlot {
        solid_va: VertexArray<BrowserGl>,
        wire_va: Option<VertexArray<BrowserGl>>,
        model_matrix: Matrix4,
        color: [f32; 4],
    }

    #[wasm_bindgen]
    pub struct LiveGlobeApp {
        context: Context<BrowserGl>,
        width: u32,
        height: u32,
        pixel_ratio: f32,
        // Programs
        globe_program: ShaderProgram<BrowserGl>,
        halo_program: ShaderProgram<BrowserGl>,
        line_program: ShaderProgram<BrowserGl>,
        mesh_program: ShaderProgram<BrowserGl>,
        star_program: ShaderProgram<BrowserGl>,
        // Core meshes
        globe_va: VertexArray<BrowserGl>,
        graticule_va: VertexArray<BrowserGl>,
        stars_va: VertexArray<BrowserGl>,
        earth_texture: Rc<Texture<BrowserGl>>,
        tiles_va: Option<VertexArray<BrowserGl>>,
        tile_key: String,
        tile_level: u32,
        // Route arcs
        geodesic_va: Option<VertexArray<BrowserGl>>,
        rhumb_va: Option<VertexArray<BrowserGl>>,
        // Spline path + sample points
        spline_va: Option<VertexArray<BrowserGl>>,
        spline_points: Vec<Cartesian3>,
        // Procedural Core geometry showcase
        geom_slot: Option<GeometrySlot>,
        geom_wireframe: bool,
        geom_kind: String,
        // DataSource overlay lines + points
        ds_lines_va: Option<VertexArray<BrowserGl>>,
        ds_points_va: Option<VertexArray<BrowserGl>>,
        // Dynamic point buffer (markers)
        marker_points: Vec<([f32; 3], [f32; 4], f32)>,
        // Camera & body state
        body: String,
        ellipsoid: Ellipsoid,
        scene_mode: i32, // 0 = 3D, 1 = 2D, 2 = Columbus
        cam_lon: f64,
        cam_lat: f64,
        cam_heading: f64,
        cam_pitch: f64,
        cam_range: f64,
        cam_fov_deg: f64,
        last_frame: Option<CameraFrame>,
        // Layers
        show_graticule: bool,
        show_tiles: bool,
        tile_scheme: u32,
        show_sun_lighting: bool,
        show_atmosphere: bool,
        show_stars: bool,
        show_rhumb: bool,
        show_spline: bool,
        // Clipping
        clip_enabled: bool,
        clip_distance_ratio: f32,
        clip_angle_deg: f32,
        // Time
        julian_date: JulianDate,
        // Post-processing
        offscreen: Option<OffscreenTarget>,
        post_process: PostProcessStageCollection<BrowserGl>,
        pp_effect: String,
        pp_fxaa: bool,
        pp_bloom: bool,
        pp_tonemapper: String,
        pp_exposure: f64,
        // Diagnostics
        frame_count: u64,
    }

    #[wasm_bindgen]
    impl LiveGlobeApp {
        #[wasm_bindgen(constructor)]
        pub fn new(gl: JsValue, width: u32, height: u32, pixel_ratio: f32) -> Result<LiveGlobeApp, JsValue> {
            let context = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
            let globe_program = ShaderProgram::new(
                &context,
                GLOBE_VS,
                GLOBE_FS,
                &[("a_position", 0), ("a_normal", 1), ("a_uv", 2)],
            )
            .map_err(err)?;
            let halo_program = ShaderProgram::new(
                &context,
                GLOBE_VS,
                HALO_FS,
                &[("a_position", 0), ("a_normal", 1), ("a_uv", 2)],
            )
            .map_err(err)?;
            let line_program = ShaderProgram::new(&context, LINE_VS, LINE_FS, &[("a_position", 0)]).map_err(err)?;
            let mesh_program =
                ShaderProgram::new(&context, MESH_VS, MESH_FS, &[("a_position", 0), ("a_normal", 1)]).map_err(err)?;
            let star_program = ShaderProgram::new(&context, STAR_VS, STAR_FS, &[("a_star", 0)]).map_err(err)?;

            let ellipsoid = Ellipsoid::WGS84;
            let globe_va = Self::build_globe_va(&context, &ellipsoid).map_err(err)?;
            let graticule_va = create_line_va(&context, &build_graticule(&ellipsoid, 15.0, 48)).map_err(err)?;
            let stars_data = build_stars(2600, 42);
            let stars_buf =
                Buffer::new(&context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(f32_bytes(&stars_data)))
                    .map_err(err)?;
            let mut star_attr = VertexAttribute::buffer(0, stars_buf, 4);
            star_attr.stride = 16;
            let stars_va = VertexArray::new(&context, vec![star_attr], None).map_err(err)?;

            let mut tex_opts = TextureOptions::rgba(1, 1);
            tex_opts.flip_y = false;
            let earth_texture =
                Rc::new(Texture::new(&context, tex_opts, Some(&[20, 44, 78, 255])).map_err(err)?);

            let post_process = PostProcessStageCollection::new(&context).map_err(err)?;
            let w = width.max(1);
            let h = height.max(1);
            let offscreen = Some(OffscreenTarget::new(&context, w, h).map_err(err)?);

            let mut app = LiveGlobeApp {
                context,
                width: w,
                height: h,
                pixel_ratio: pixel_ratio.max(1.0),
                globe_program,
                halo_program,
                line_program,
                mesh_program,
                star_program,
                globe_va,
                graticule_va,
                stars_va,
                earth_texture,
                tiles_va: None,
                tile_key: String::new(),
                tile_level: 0,
                geodesic_va: None,
                rhumb_va: None,
                spline_va: None,
                spline_points: Vec::new(),
                geom_slot: None,
                geom_wireframe: false,
                geom_kind: "none".into(),
                ds_lines_va: None,
                ds_points_va: None,
                marker_points: Vec::new(),
                body: "wgs84".into(),
                ellipsoid,
                scene_mode: 0,
                cam_lon: to_radians(-40.0),
                cam_lat: to_radians(28.0),
                cam_heading: 0.0,
                cam_pitch: -PI_OVER_TWO,
                cam_range: 19_500_000.0,
                cam_fov_deg: 50.0,
                last_frame: None,
                show_graticule: true,
                show_tiles: false,
                tile_scheme: 0,
                show_sun_lighting: true,
                show_atmosphere: true,
                show_stars: true,
                show_rhumb: true,
                show_spline: false,
                clip_enabled: false,
                clip_distance_ratio: 0.0,
                clip_angle_deg: 0.0,
                julian_date: JulianDate::from_date(js_sys::Date::now()),
                offscreen,
                post_process,
                pp_effect: "none".into(),
                pp_fxaa: false,
                pp_bloom: false,
                pp_tonemapper: "none".into(),
                pp_exposure: 1.0,
                frame_count: 0,
            };
            app.set_route(-74.006, 40.7128, -0.1276, 51.5074)?;
            Ok(app)
        }

        fn build_globe_va(context: &Context<BrowserGl>, ellipsoid: &Ellipsoid) -> Result<VertexArray<BrowserGl>, RuntimeError> {
            let mesh = build_globe(ellipsoid, 96, 192);
            let vbuf = Buffer::new(
                context,
                BufferKind::Vertex,
                BufferUsage::StaticDraw,
                BufferData::Bytes(f32_bytes(&mesh.vertices)),
            )?;
            let ibuf = Buffer::new(
                context,
                BufferKind::Index(IndexDatatype::UnsignedInt),
                BufferUsage::StaticDraw,
                BufferData::Bytes(u32_bytes(&mesh.indices)),
            )?;
            let mut pos = VertexAttribute::buffer(0, vbuf.clone(), 3);
            pos.stride = 32;
            pos.offset = 0;
            let mut norm = VertexAttribute::buffer(1, vbuf.clone(), 3);
            norm.stride = 32;
            norm.offset = 12;
            let mut uv = VertexAttribute::buffer(2, vbuf, 2);
            uv.stride = 32;
            uv.offset = 24;
            VertexArray::new(context, vec![pos, norm, uv], Some(ibuf))
        }

        #[wasm_bindgen(js_name = setEarthImage)]
        pub fn set_earth_image(&mut self, image: JsValue, width: u32, height: u32) -> Result<(), JsValue> {
            let mut opts = TextureOptions::rgba(width, height);
            opts.flip_y = false;
            opts.sampler = Sampler {
                wrap_s: TextureWrap::Repeat,
                wrap_t: TextureWrap::ClampToEdge,
                minification_filter: TextureMinificationFilter::LinearMipmapLinear,
                magnification_filter: TextureMagnificationFilter::Linear,
                maximum_anisotropy: 4.0,
                ..Default::default()
            };
            let tex = Texture::from_image(&self.context, opts, image).map_err(err)?;
            tex.generate_mipmap(MipmapHint::Nicest).map_err(err)?;
            self.earth_texture = Rc::new(tex);
            Ok(())
        }

        pub fn resize(&mut self, width: u32, height: u32, pixel_ratio: f32) -> Result<(), JsValue> {
            let w = width.max(1);
            let h = height.max(1);
            self.pixel_ratio = pixel_ratio.max(1.0);
            if self.width != w || self.height != h {
                self.width = w;
                self.height = h;
                self.offscreen = Some(OffscreenTarget::new(&self.context, w, h).map_err(err)?);
            }
            Ok(())
        }

        #[wasm_bindgen(js_name = setCamera)]
        pub fn set_camera(&mut self, lon_rad: f64, lat_rad: f64, heading: f64, pitch: f64, range: f64, fov_deg: f64) {
            self.cam_lon = lon_rad;
            self.cam_lat = lat_rad;
            self.cam_heading = heading;
            self.cam_pitch = pitch;
            self.cam_range = range.max(self.ellipsoid.maximum_radius * 0.04);
            self.cam_fov_deg = fov_deg.clamp(20.0, 100.0);
        }

        #[wasm_bindgen(js_name = setBody)]
        pub fn set_body(&mut self, body: &str) -> Result<String, JsValue> {
            self.body = body.to_owned();
            let old_r = self.ellipsoid.maximum_radius;
            self.ellipsoid = ellipsoid_by_name(body);
            let new_r = self.ellipsoid.maximum_radius;
            self.cam_range = (self.cam_range / old_r) * new_r;
            self.globe_va = Self::build_globe_va(&self.context, &self.ellipsoid).map_err(err)?;
            self.graticule_va = create_line_va(&self.context, &build_graticule(&self.ellipsoid, 15.0, 48)).map_err(err)?;
            self.tile_key.clear();
            if self.geom_kind != "none" {
                let kind = self.geom_kind.clone();
                let wf = self.geom_wireframe;
                self.set_geometry_showcase(&kind, wf)?;
            }
            Ok(json!({
                "body": self.body,
                "radii": [self.ellipsoid.radii.x, self.ellipsoid.radii.y, self.ellipsoid.radii.z],
                "range": self.cam_range,
            })
            .to_string())
        }

        #[wasm_bindgen(js_name = setSceneMode)]
        pub fn set_scene_mode(&mut self, mode: i32) {
            self.scene_mode = mode.clamp(0, 2);
        }

        #[wasm_bindgen(js_name = setLayers)]
        pub fn set_layers(
            &mut self,
            graticule: bool,
            tiles: bool,
            tile_scheme: u32,
            sun_lighting: bool,
            atmosphere: bool,
            stars: bool,
            rhumb: bool,
            spline: bool,
        ) {
            self.show_graticule = graticule;
            self.show_tiles = tiles;
            if self.tile_scheme != tile_scheme {
                self.tile_scheme = tile_scheme;
                self.tile_key.clear();
            }
            self.show_sun_lighting = sun_lighting;
            self.show_atmosphere = atmosphere;
            self.show_stars = stars;
            self.show_rhumb = rhumb;
            self.show_spline = spline;
        }

        #[wasm_bindgen(js_name = setClipping)]
        pub fn set_clipping(&mut self, enabled: bool, distance_ratio: f32, angle_deg: f32) {
            self.clip_enabled = enabled;
            self.clip_distance_ratio = distance_ratio;
            self.clip_angle_deg = angle_deg;
        }

        #[wasm_bindgen(js_name = setJulianDate)]
        pub fn set_julian_date(&mut self, jd_days: f64) {
            let day = jd_days.floor();
            let sec = (jd_days - day) * 86400.0;
            self.julian_date = JulianDate::new(day, sec, TimeStandard::Utc);
        }

        #[wasm_bindgen(js_name = setPostProcess)]
        pub fn set_post_process(
            &mut self,
            effect: &str,
            fxaa: bool,
            bloom: bool,
            tonemapper: &str,
            exposure: f64,
        ) -> Result<(), JsValue> {
            self.pp_effect = effect.to_owned();
            self.pp_fxaa = fxaa;
            self.pp_bloom = bloom;
            self.pp_tonemapper = tonemapper.to_owned();
            self.pp_exposure = exposure;

            self.post_process.remove_all().map_err(err)?;
            self.post_process.fxaa_mut().set_enabled(fxaa);
            self.post_process.bloom_mut().set_enabled(bloom);
            self.post_process.set_exposure(exposure);
            match tonemapper {
                "aces" => self.post_process.set_tonemapper(Tonemapper::Aces).map_err(err)?,
                "filmic" => self.post_process.set_tonemapper(Tonemapper::Filmic).map_err(err)?,
                "pbr_neutral" => self.post_process.set_tonemapper(Tonemapper::PbrNeutral).map_err(err)?,
                "reinhard" => self.post_process.set_tonemapper(Tonemapper::Reinhard).map_err(err)?,
                "modified_reinhard" => self.post_process.set_tonemapper(Tonemapper::ModifiedReinhard).map_err(err)?,
                _ => {}
            }
            match effect {
                "bw" => {
                    let s = PostProcessStageLibrary::create_black_and_white_stage().map_err(err)?;
                    self.post_process.add(s).map_err(err)?;
                }
                "night_vision" => {
                    let s = PostProcessStageLibrary::create_night_vision_stage().map_err(err)?;
                    self.post_process.add(s).map_err(err)?;
                }
                "brightness" => {
                    let s = PostProcessStageLibrary::create_brightness_stage().map_err(err)?;
                    self.post_process.add(s).map_err(err)?;
                }
                "blur" => {
                    let s = PostProcessStageLibrary::create_blur_stage("live_blur").map_err(err)?;
                    self.post_process.add(s).map_err(err)?;
                }
                "lens_flare" => {
                    let s = PostProcessStageLibrary::create_lens_flare_stage().map_err(err)?;
                    self.post_process.add(s).map_err(err)?;
                }
                _ => {}
            }
            Ok(())
        }

        #[wasm_bindgen(js_name = setRoute)]
        pub fn set_route(&mut self, lon1_deg: f64, lat1_deg: f64, lon2_deg: f64, lat2_deg: f64) -> Result<String, JsValue> {
            let (lon1, lat1) = (to_radians(lon1_deg), to_radians(lat1_deg));
            let (lon2, lat2) = (to_radians(lon2_deg), to_radians(lat2_deg));
            let geo_arc = build_geodesic_arc(&self.ellipsoid, lon1, lat1, lon2, lat2, 160);
            self.geodesic_va = Some(create_line_va(&self.context, &geo_arc).map_err(err)?);

            let start = Cartographic::new(lon1, lat1, 0.0);
            let end = Cartographic::new(lon2, lat2, 0.0);
            let rhumb = EllipsoidRhumbLine::new(&start, &end, &self.ellipsoid);
            let segs = 128usize;
            let mut rhumb_lines = Vec::with_capacity(segs * 6);
            let mut prev = self.ellipsoid.cartographic_to_cartesian(&rhumb.interpolate_using_fraction(0.0));
            for i in 1..=segs {
                let c = rhumb.interpolate_using_fraction(i as f64 / segs as f64);
                let curr = self.ellipsoid.cartographic_to_cartesian(&c);
                rhumb_lines.extend_from_slice(&[
                    prev.x as f32,
                    prev.y as f32,
                    prev.z as f32,
                    curr.x as f32,
                    curr.y as f32,
                    curr.z as f32,
                ]);
                prev = curr;
            }
            self.rhumb_va = Some(create_line_va(&self.context, &rhumb_lines).map_err(err)?);

            let geo = Geodesic::new(&self.ellipsoid, lon1, lat1, lon2, lat2);
            let (mid_lon, mid_lat) = geo.interpolate(0.5);
            Ok(json!({
                "geodesicDistanceKm": geo.distance / 1000.0,
                "geodesicStartHeadingDeg": to_degrees(geo.start_heading),
                "geodesicEndHeadingDeg": to_degrees(geo.end_heading),
                "midpointDeg": [to_degrees(mid_lon), to_degrees(mid_lat)],
                "rhumbDistanceKm": rhumb.surface_distance / 1000.0,
                "rhumbHeadingDeg": to_degrees(rhumb.heading),
                "deltaKm": (rhumb.surface_distance - geo.distance) / 1000.0,
            })
            .to_string())
        }

        #[wasm_bindgen(js_name = setSplineMode)]
        pub fn set_spline_mode(&mut self, mode: &str) -> Result<String, JsValue> {
            let e = &self.ellipsoid;
            let alt = e.maximum_radius * 0.08;
            let waypoints = [
                (-122.4, 37.77, alt * 0.4),
                (-105.0, 40.0, alt * 1.1),
                (-74.0, 40.71, alt * 0.6),
                (-30.0, 52.0, alt * 1.3),
                (-0.12, 51.5, alt * 0.5),
                (13.4, 52.5, alt * 0.9),
                (37.6, 55.75, alt * 0.7),
            ];
            let pts: Vec<Cartesian3> = waypoints
                .iter()
                .map(|&(lon, lat, h)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, h)))
                .collect();
            let times: Vec<f64> = (0..pts.len()).map(|i| i as f64).collect();
            let steps = 240usize;
            let mut sampled = Vec::with_capacity(steps + 1);
            let max_t = (pts.len() - 1) as f64;
            match mode {
                "linear" => {
                    let s = LinearSpline::new(times, pts).ok_or_else(|| err("Invalid LinearSpline"))?;
                    for i in 0..=steps {
                        if let Some(p) = s.evaluate(max_t * i as f64 / steps as f64) {
                            sampled.push(p);
                        }
                    }
                }
                "hermite" => {
                    let s = HermiteSpline::create_natural_cubic(times, pts)
                        .ok_or_else(|| err("Invalid HermiteSpline"))?;
                    for i in 0..=steps {
                        if let Some(p) = s.evaluate(max_t * i as f64 / steps as f64) {
                            sampled.push(p);
                        }
                    }
                }
                _ => {
                    let s = CatmullRomSpline::new(times, pts, None, None)
                        .ok_or_else(|| err("Invalid CatmullRomSpline"))?;
                    for i in 0..=steps {
                        if let Some(p) = s.evaluate(max_t * i as f64 / steps as f64) {
                            sampled.push(p);
                        }
                    }
                }
            }
            let mut lines = Vec::with_capacity(steps * 6);
            let mut total_len = 0.0;
            for w in sampled.windows(2) {
                total_len += (w[1] - w[0]).magnitude();
                lines.extend_from_slice(&[
                    w[0].x as f32,
                    w[0].y as f32,
                    w[0].z as f32,
                    w[1].x as f32,
                    w[1].y as f32,
                    w[1].z as f32,
                ]);
            }
            self.spline_va = Some(create_line_va(&self.context, &lines).map_err(err)?);
            self.spline_points = sampled;
            Ok(json!({
                "mode": mode,
                "samples": steps + 1,
                "arcLengthKm": total_len / 1000.0,
            })
            .to_string())
        }

        #[wasm_bindgen(js_name = setGeometryShowcase)]
        pub fn set_geometry_showcase(&mut self, kind: &str, wireframe: bool) -> Result<String, JsValue> {
            self.geom_kind = kind.to_owned();
            self.geom_wireframe = wireframe;
            if kind == "none" {
                self.geom_slot = None;
                return Ok(json!({"kind": "none"}).to_string());
            }
            let e = self.ellipsoid;
            let r = e.maximum_radius;
            let scale = r / 6_378_137.0;
            let center_cart = Cartographic::from_degrees(-105.0, 39.5, 350_000.0 * scale);
            let center = e.cartographic_to_cartesian(&center_cart);
            let enu = Transforms::east_north_up_to_fixed_frame(center, &e);
            let (geom, model_matrix, color): (Option<Geometry>, Matrix4, [f32; 4]) = match kind {
                "box" => (
                    BoxGeometry::from_dimensions(
                        Cartesian3::new(900_000.0 * scale, 650_000.0 * scale, 500_000.0 * scale),
                        VertexFormat::POSITION_AND_NORMAL,
                        None,
                    )
                    .and_then(|g| g.create_geometry()),
                    enu,
                    [0.95, 0.65, 0.18, 0.92],
                ),
                "sphere" => (
                    SphereGeometry::new(500_000.0 * scale, 32.0, 24.0, VertexFormat::POSITION_AND_NORMAL)
                        .and_then(|g| g.create_geometry()),
                    enu,
                    [0.24, 0.78, 0.96, 0.92],
                ),
                "ellipsoid" => (
                    EllipsoidGeometry::new(
                        Cartesian3::new(750_000.0 * scale, 480_000.0 * scale, 360_000.0 * scale),
                        None,
                        0.0,
                        2.0 * PI,
                        0.0,
                        PI,
                        32.0,
                        24.0,
                        VertexFormat::POSITION_AND_NORMAL,
                        None,
                    )
                    .and_then(|g| g.create_geometry()),
                    enu,
                    [0.62, 0.45, 0.96, 0.92],
                ),
                "cylinder" => (
                    CylinderGeometry::new(
                        800_000.0 * scale,
                        180_000.0 * scale,
                        480_000.0 * scale,
                        36,
                        VertexFormat::POSITION_AND_NORMAL,
                        None,
                    )
                    .and_then(|g| g.create_geometry()),
                    enu,
                    [0.26, 0.88, 0.56, 0.92],
                ),
                "polygon" => {
                    let outer: Vec<Cartesian3> = [
                        (-115.0, 32.0),
                        (-95.0, 32.0),
                        (-92.0, 44.0),
                        (-105.0, 48.0),
                        (-118.0, 43.0),
                    ]
                    .iter()
                    .map(|&(lon, lat)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
                    .collect();
                    let hole: Vec<Cartesian3> = [(-108.0, 36.0), (-102.0, 36.0), (-102.0, 41.0), (-108.0, 41.0)]
                        .iter()
                        .map(|&(lon, lat)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
                        .collect();
                    let mut opts = PolygonGeometryOptions::new(PolygonHierarchy::new(
                        outer,
                        vec![PolygonHierarchy::new(hole, vec![])],
                    ));
                    opts.ellipsoid = e;
                    opts.height = 100_000.0 * scale;
                    opts.extruded_height = Some(420_000.0 * scale);
                    opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                    (PolygonGeometry::new(opts).create_geometry(), Matrix4::IDENTITY, [0.96, 0.36, 0.52, 0.92])
                }
                "corridor" => {
                    let pts: Vec<Cartesian3> = [(-120.0, 35.0), (-105.0, 41.0), (-88.0, 36.0), (-74.0, 41.0)]
                        .iter()
                        .map(|&(lon, lat)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
                        .collect();
                    let mut opts = CorridorGeometryOptions::new(pts, 350_000.0 * scale);
                    opts.ellipsoid = e;
                    opts.height = 120_000.0 * scale;
                    opts.extruded_height = Some(260_000.0 * scale);
                    opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                    (CorridorGeometry::new(opts).create_geometry(), Matrix4::IDENTITY, [0.98, 0.78, 0.22, 0.92])
                }
                "wall" => {
                    let pts: Vec<Cartesian3> = [(-122.0, 37.0), (-110.0, 42.0), (-96.0, 38.0), (-82.0, 43.0)]
                        .iter()
                        .map(|&(lon, lat)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
                        .collect();
                    let g = WallGeometry::from_constant_heights(
                        pts,
                        Some(40_000.0 * scale),
                        Some(520_000.0 * scale),
                        e,
                        VertexFormat::POSITION_AND_NORMAL,
                    );
                    (g.create_geometry(), Matrix4::IDENTITY, [0.22, 0.86, 0.82, 0.92])
                }
                "rectangle" => {
                    let mut opts = RectangleGeometryOptions::new(Rectangle::from_degrees(-116.0, 31.0, -94.0, 46.0));
                    opts.ellipsoid = e;
                    opts.height = 80_000.0 * scale;
                    opts.extruded_height = Some(300_000.0 * scale);
                    opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                    (RectangleGeometry::new(opts).create_geometry(), Matrix4::IDENTITY, [0.35, 0.65, 0.98, 0.92])
                }
                "circle" => {
                    let mut opts = EllipseGeometryOptions::new(center, 700_000.0 * scale, 700_000.0 * scale);
                    opts.ellipsoid = e;
                    opts.height = 90_000.0 * scale;
                    opts.extruded_height = Some(340_000.0 * scale);
                    opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                    (
                        CircleGeometry::new(700_000.0 * scale, opts).and_then(|g| g.create_geometry()),
                        Matrix4::IDENTITY,
                        [0.98, 0.52, 0.25, 0.92],
                    )
                }
                "frustum" => {
                    let f = Frustum::Perspective(PerspectiveFrustum {
                        fov: to_radians(45.0),
                        aspect_ratio: 1.5,
                        near: 250_000.0 * scale,
                        far: 1_100_000.0 * scale,
                        x_offset: 0.0,
                        y_offset: 0.0,
                    });
                    let fg = FrustumGeometry::new(
                        f,
                        Cartesian3::ZERO,
                        Quaternion::IDENTITY,
                        VertexFormat::POSITION_AND_NORMAL,
                    );
                    (fg.create_geometry(), enu, [0.85, 0.45, 0.98, 0.92])
                }
                _ => (None, Matrix4::IDENTITY, [1.0, 1.0, 1.0, 1.0]),
            };
            let geom = geom.ok_or_else(|| err(format!("Failed to build geometry '{kind}'")))?;
            let (interleaved, indices, wire_indices) =
                extract_geometry_pos_norm_idx(&geom).ok_or_else(|| err("Geometry missing position data"))?;
            let vertex_count = interleaved.len() / 6;
            let triangle_count = indices.len() / 3;
            let solid_va = create_mesh_va(&self.context, &interleaved, &indices).map_err(err)?;
            let wire_va = if !wire_indices.is_empty() {
                Some(create_mesh_va(&self.context, &interleaved, &wire_indices).map_err(err)?)
            } else {
                None
            };
            let bs = geom.bounding_sphere.map(|b| json!({"center": [b.center.x, b.center.y, b.center.z], "radius": b.radius}));
            self.geom_slot = Some(GeometrySlot { solid_va, wire_va, model_matrix, color });
            Ok(json!({
                "kind": kind,
                "vertices": vertex_count,
                "triangles": triangle_count,
                "wireframeSegments": wire_indices.len() / 2,
                "boundingSphere": bs,
            })
            .to_string())
        }

        #[wasm_bindgen(js_name = setDataSourceOverlay)]
        pub fn set_data_source_overlay(&mut self, overlay_json: &str) -> Result<(), JsValue> {
            let v: Value = serde_json::from_str(overlay_json).map_err(err)?;
            let mut lines = Vec::<f32>::new();
            let mut pts = Vec::<f32>::new();
            if let Some(arr) = v["polylines"].as_array() {
                for poly in arr {
                    if let Some(coords) = poly.as_array() {
                        let c3s: Vec<[f32; 3]> = coords
                            .iter()
                            .filter_map(|p| {
                                let a = p.as_array()?;
                                Some([a[0].as_f64()? as f32, a[1].as_f64()? as f32, a[2].as_f64()? as f32])
                            })
                            .collect();
                        for w in c3s.windows(2) {
                            lines.extend_from_slice(&w[0]);
                            lines.extend_from_slice(&w[1]);
                        }
                    }
                }
            }
            if let Some(arr) = v["points"].as_array() {
                for p in arr {
                    if let Some(a) = p.as_array() {
                        if a.len() >= 3 {
                            pts.push(a[0].as_f64().unwrap_or(0.0) as f32);
                            pts.push(a[1].as_f64().unwrap_or(0.0) as f32);
                            pts.push(a[2].as_f64().unwrap_or(0.0) as f32);
                        }
                    }
                }
            }
            self.ds_lines_va = if !lines.is_empty() {
                Some(create_line_va(&self.context, &lines).map_err(err)?)
            } else {
                None
            };
            self.ds_points_va = if !pts.is_empty() {
                Some(create_line_va(&self.context, &pts).map_err(err)?)
            } else {
                None
            };
            Ok(())
        }

        #[wasm_bindgen(js_name = setMarkers)]
        pub fn set_markers(&mut self, markers_json: &str) -> Result<(), JsValue> {
            let arr: Vec<Value> = serde_json::from_str(markers_json).map_err(err)?;
            self.marker_points.clear();
            for m in arr {
                let lon = m["lon"].as_f64().unwrap_or(0.0);
                let lat = m["lat"].as_f64().unwrap_or(0.0);
                let h = m["height"].as_f64().unwrap_or(0.0);
                let p = self.ellipsoid.cartographic_to_cartesian(&Cartographic::new(lon, lat, h));
                let col = m["color"]
                    .as_array()
                    .map(|a| {
                        [
                            a.first().and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                            a.get(1).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                            a.get(2).and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
                            1.0,
                        ]
                    })
                    .unwrap_or([1.0, 0.8, 0.2, 1.0]);
                let size = m["size"].as_f64().unwrap_or(8.0) as f32;
                self.marker_points.push(([p.x as f32, p.y as f32, p.z as f32], col, size));
            }
            Ok(())
        }

        #[wasm_bindgen(js_name = pickNdc)]
        pub fn pick_ndc(&mut self, ndc_x: f64, ndc_y: f64) -> String {
            let frame = self.compute_frame();
            let (origin, direction) = ray_from_ndc(&frame, ndc_x, ndc_y);
            match pick_ray(origin, direction, &self.ellipsoid) {
                Some(hit) => json!({
                    "hit": true,
                    "lonRad": hit.longitude,
                    "latRad": hit.latitude,
                    "lonDeg": to_degrees(hit.longitude),
                    "latDeg": to_degrees(hit.latitude),
                    "height": hit.height,
                    "ecef": [hit.position.x, hit.position.y, hit.position.z],
                    "distanceKm": hit.t / 1000.0,
                })
                .to_string(),
                None => json!({"hit": false}).to_string(),
            }
        }

        fn fov_y(&self) -> f64 {
            let aspect = self.width as f64 / self.height.max(1) as f64;
            let base = to_radians(self.cam_fov_deg);
            if aspect >= 1.0 {
                base
            } else {
                2.0 * ((base / 2.0).tan() / aspect).atan()
            }
        }

        fn compute_frame(&self) -> CameraFrame {
            let aspect = self.width as f64 / self.height.max(1) as f64;
            let r = self.ellipsoid.maximum_radius;
            let height = self.cam_range * (-self.cam_pitch).sin();
            let near = (height * 0.05).max(r * 0.001);
            let far = (r + height) + r * 3.0;
            if self.scene_mode == 0 {
                look_at(
                    &self.ellipsoid,
                    self.cam_lon,
                    self.cam_lat,
                    0.0,
                    self.cam_heading,
                    self.cam_pitch,
                    self.cam_range,
                    self.fov_y(),
                    aspect,
                    near,
                    far,
                )
            } else {
                // 2D or Columbus View flat camera looking at projected (lon*R, lat*R, 0)
                let focus = Cartesian3::new(self.cam_lon * r, self.cam_lat * r, 0.0);
                let pitch = if self.scene_mode == 1 { -PI_OVER_TWO + 0.001 } else { self.cam_pitch.max(-to_radians(55.0)) };
                let dist = self.cam_range;
                let eye = Cartesian3::new(
                    focus.x - dist * pitch.cos() * self.cam_heading.sin(),
                    focus.y - dist * pitch.cos() * self.cam_heading.cos(),
                    dist * (-pitch).sin(),
                );
                let direction = (focus - eye).normalize();
                let up_ref = if self.scene_mode == 1 {
                    Cartesian3::new(self.cam_heading.sin(), self.cam_heading.cos(), 0.0)
                } else {
                    Cartesian3::UNIT_Z
                };
                let right = direction.cross(up_ref).normalize();
                let up = right.cross(direction).normalize();
                let view = Matrix4::compute_view(eye, direction, up, right);
                let projection = PerspectiveFrustum {
                    fov: self.fov_y(),
                    aspect_ratio: aspect,
                    near,
                    far: far * 4.0,
                    x_offset: 0.0,
                    y_offset: 0.0,
                }
                .projection_matrix();
                let view_projection = projection.multiply(&view);
                CameraFrame {
                    view,
                    projection,
                    view_projection,
                    eye,
                    direction,
                    up,
                    right,
                    focus,
                    fov_y: self.fov_y(),
                    aspect,
                }
            }
        }

        fn update_tiles(&mut self) -> Result<(), RuntimeError> {
            if !self.show_tiles || self.scene_mode != 0 {
                return Ok(());
            }
            let level = suggested_tile_level(self.cam_range, self.ellipsoid.maximum_radius);
            let key = format!(
                "{}:{}:{}:{}",
                level,
                self.tile_scheme,
                to_degrees(self.cam_lon).round() as i32,
                to_degrees(self.cam_lat).round() as i32
            );
            if key == self.tile_key {
                return Ok(());
            }
            self.tile_key = key;
            self.tile_level = level;
            let segs = (64u32 >> level.min(6)).max(4);
            let lines = build_tile_grid(&self.ellipsoid, level, self.tile_scheme, self.cam_lon, self.cam_lat, 3, segs);
            self.tiles_va = Some(create_line_va(&self.context, &lines)?);
            Ok(())
        }

        fn post_process_active(&self) -> bool {
            self.pp_effect != "none" || self.pp_fxaa || self.pp_bloom || self.pp_tonemapper != "none"
        }

        pub fn render(&mut self, anim_time_sec: f64) -> Result<String, JsValue> {
            self.frame_count += 1;
            self.update_tiles().map_err(err)?;
            let frame = self.compute_frame();
            self.last_frame = Some(frame);

            let vp = Viewport { x: 0, y: 0, width: self.width, height: self.height };
            let use_pp = self.post_process_active();
            let target_fb = if use_pp {
                self.offscreen.as_ref().map(|o| o.framebuffer.clone())
            } else {
                None
            };
            let pass_state = PassState { viewport: Some(vp), framebuffer: target_fb.clone(), ..Default::default() };

            // Update UniformState for PostProcess stages (needs czm_viewport, etc.)
            let mut cam = Camera::new(CameraOptions::new(self.width as f64, self.height as f64));
            cam.position = frame.eye;
            cam.direction = frame.direction;
            cam.up = frame.up;
            cam.right = frame.right;
            let uf = UniformFrame::new(cam.uniform_camera(), self.julian_date);
            self.context.uniform_state().update(&uf, self.width).map_err(err)?;

            ClearCommand {
                color: Some([0.024, 0.032, 0.048, 1.0]),
                depth: Some(1.0),
                stencil: Some(0),
                framebuffer: target_fb.clone(),
                ..Default::default()
            }
            .execute(&self.context, &pass_state)
            .map_err(err)?;

            let view_f32 = mat4_f32(&frame.view.0);
            let proj_f32 = mat4_f32(&frame.projection.0);
            let vp_f32 = mat4_f32(&frame.view_projection.0);
            let sun_dir = sun_direction_ecef(self.julian_date.total_days());
            let body_kind = match self.body.as_str() {
                "moon" => 1i32,
                "mars" => 2i32,
                "unit" => 3i32,
                _ => 0i32,
            };

            // 1. Stars
            if self.show_stars && self.scene_mode == 0 {
                let mut cmd = DrawCommand::new(&self.stars_va, &self.star_program);
                cmd.primitive_type = PrimitiveType::Points;
                cmd.render_state.depth_test.enabled = false;
                cmd.render_state.depth_mask = false;
                cmd.uniforms = vec![
                    ("u_view".into(), mat4_uniform(view_f32.clone())),
                    ("u_projection".into(), mat4_uniform(proj_f32.clone())),
                    ("u_pixelRatio".into(), UniformValue::Float(vec![self.pixel_ratio])),
                ];
                cmd.execute(&self.context, &pass_state).map_err(err)?;
            }

            // Clip plane calculation
            let clip_rad = to_radians(f64::from(self.clip_angle_deg));
            let clip_normal = [clip_rad.cos() as f32, clip_rad.sin() as f32, 0.25f32];
            let cn_len = (clip_normal[0] * clip_normal[0] + clip_normal[1] * clip_normal[1] + clip_normal[2] * clip_normal[2]).sqrt();
            let clip_plane = [
                clip_normal[0] / cn_len,
                clip_normal[1] / cn_len,
                clip_normal[2] / cn_len,
                self.clip_distance_ratio * self.ellipsoid.maximum_radius as f32,
            ];

            // 2. Globe surface
            {
                let mut cmd = DrawCommand::new(&self.globe_va, &self.globe_program);
                cmd.primitive_type = PrimitiveType::Triangles;
                cmd.render_state.depth_test.enabled = true;
                cmd.render_state.depth_mask = true;
                cmd.render_state.cull.enabled = !self.clip_enabled && self.scene_mode == 0;
                cmd.render_state.cull.face = CullFace::Back.value();
                cmd.textures = vec![("u_texture".into(), &*self.earth_texture)];
                cmd.uniforms = vec![
                    ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                    ("u_scale".into(), UniformValue::Float(vec![1.0])),
                    ("u_sceneMode".into(), UniformValue::Int(vec![self.scene_mode])),
                    ("u_bodyRadius".into(), UniformValue::Float(vec![self.ellipsoid.maximum_radius as f32])),
                    ("u_eye".into(), UniformValue::Float(vec![frame.eye.x as f32, frame.eye.y as f32, frame.eye.z as f32])),
                    ("u_sun".into(), UniformValue::Float(vec![sun_dir.x as f32, sun_dir.y as f32, sun_dir.z as f32])),
                    ("u_sunLighting".into(), UniformValue::Float(vec![if self.show_sun_lighting { 1.0 } else { 0.0 }])),
                    ("u_bodyKind".into(), UniformValue::Int(vec![body_kind])),
                    ("u_clipEnabled".into(), UniformValue::Float(vec![if self.clip_enabled { 1.0 } else { 0.0 }])),
                    ("u_clipPlane".into(), UniformValue::Float(clip_plane.to_vec())),
                ];
                cmd.execute(&self.context, &pass_state).map_err(err)?;
            }

            // 3. Atmosphere Halo (3D Earth / Mars)
            if self.show_atmosphere && self.scene_mode == 0 && (body_kind == 0 || body_kind == 2) && !self.clip_enabled {
                let mut cmd = DrawCommand::new(&self.globe_va, &self.halo_program);
                cmd.primitive_type = PrimitiveType::Triangles;
                cmd.render_state.depth_test.enabled = true;
                cmd.render_state.depth_mask = false;
                cmd.render_state.cull.enabled = true;
                cmd.render_state.cull.face = CullFace::Front.value();
                cmd.render_state.blending.enabled = true;
                cmd.render_state.blending.function_source_rgb = BlendFunction::One.value();
                cmd.render_state.blending.function_destination_rgb = BlendFunction::One.value();
                cmd.render_state.blending.function_source_alpha = BlendFunction::One.value();
                cmd.render_state.blending.function_destination_alpha = BlendFunction::One.value();
                cmd.uniforms = vec![
                    ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                    ("u_scale".into(), UniformValue::Float(vec![1.028])),
                    ("u_sceneMode".into(), UniformValue::Int(vec![0])),
                    ("u_bodyRadius".into(), UniformValue::Float(vec![self.ellipsoid.maximum_radius as f32])),
                    ("u_eye".into(), UniformValue::Float(vec![frame.eye.x as f32, frame.eye.y as f32, frame.eye.z as f32])),
                    ("u_bodyKind".into(), UniformValue::Int(vec![body_kind])),
                ];
                cmd.execute(&self.context, &pass_state).map_err(err)?;
            }

            // 4. 3D Core Geometry Showcase
            if self.scene_mode == 0 {
                if let Some(slot) = &self.geom_slot {
                    let mut cmd = DrawCommand::new(&slot.solid_va, &self.mesh_program);
                    cmd.primitive_type = PrimitiveType::Triangles;
                    cmd.render_state.depth_test.enabled = true;
                    cmd.render_state.depth_mask = true;
                    cmd.render_state.blending = blending_state::alpha_blend();
                    cmd.uniforms = vec![
                        ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                        ("u_model".into(), mat4_uniform(mat4_f32(&slot.model_matrix.0))),
                        ("u_color".into(), UniformValue::Float(slot.color.to_vec())),
                        ("u_sun".into(), UniformValue::Float(vec![sun_dir.x as f32, sun_dir.y as f32, sun_dir.z as f32])),
                        ("u_eye".into(), UniformValue::Float(vec![frame.eye.x as f32, frame.eye.y as f32, frame.eye.z as f32])),
                        ("u_clipEnabled".into(), UniformValue::Float(vec![if self.clip_enabled { 1.0 } else { 0.0 }])),
                        ("u_clipPlane".into(), UniformValue::Float(clip_plane.to_vec())),
                    ];
                    if !self.geom_wireframe {
                        cmd.execute(&self.context, &pass_state).map_err(err)?;
                    }
                    if let Some(wva) = &slot.wire_va {
                        if self.geom_wireframe {
                            let mut wcmd = DrawCommand::new(wva, &self.mesh_program);
                            wcmd.primitive_type = PrimitiveType::Lines;
                            wcmd.render_state.depth_test.enabled = true;
                            wcmd.uniforms = cmd.uniforms.clone();
                            wcmd.uniforms[2] = ("u_color".into(), UniformValue::Float(vec![0.95, 0.98, 1.0, 1.0]));
                            wcmd.execute(&self.context, &pass_state).map_err(err)?;
                        }
                    }
                }
            }

            // Helper to draw 3D lines
            let draw_lines = |va: &VertexArray<BrowserGl>, scale: f32, color: [f32; 4]| -> Result<(), RuntimeError> {
                let mut cmd = DrawCommand::new(va, &self.line_program);
                cmd.primitive_type = PrimitiveType::Lines;
                cmd.render_state.depth_test.enabled = true;
                cmd.render_state.depth_mask = false;
                cmd.render_state.blending = blending_state::alpha_blend();
                cmd.uniforms = vec![
                    ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                    ("u_scale".into(), UniformValue::Float(vec![scale])),
                    ("u_color".into(), UniformValue::Float(color.to_vec())),
                    ("u_pointSize".into(), UniformValue::Float(vec![1.0])),
                    ("u_round".into(), UniformValue::Float(vec![0.0])),
                ];
                cmd.execute(&self.context, &pass_state)
            };

            // 5. Overlays (3D mode)
            if self.scene_mode == 0 {
                if self.show_graticule {
                    draw_lines(&self.graticule_va, 1.0008, [0.82, 0.90, 0.98, 0.24]).map_err(err)?;
                }
                if self.show_tiles {
                    if let Some(tva) = &self.tiles_va {
                        draw_lines(tva, 1.0012, [0.30, 0.86, 0.98, 0.62]).map_err(err)?;
                    }
                }
                if let Some(gva) = &self.geodesic_va {
                    draw_lines(gva, 1.0022, [0.96, 0.70, 0.16, 1.0]).map_err(err)?;
                }
                if self.show_rhumb {
                    if let Some(rva) = &self.rhumb_va {
                        draw_lines(rva, 1.0020, [0.94, 0.32, 0.72, 0.92]).map_err(err)?;
                    }
                }
                if self.show_spline {
                    if let Some(sva) = &self.spline_va {
                        draw_lines(sva, 1.0, [0.22, 0.95, 0.62, 0.95]).map_err(err)?;
                    }
                    if !self.spline_points.is_empty() {
                        let idx = ((anim_time_sec * 35.0) as usize) % self.spline_points.len();
                        let p = self.spline_points[idx];
                        let pva = create_line_va(&self.context, &[p.x as f32, p.y as f32, p.z as f32]).map_err(err)?;
                        let mut pcmd = DrawCommand::new(&pva, &self.line_program);
                        pcmd.primitive_type = PrimitiveType::Points;
                        pcmd.render_state.depth_test.enabled = true;
                        pcmd.render_state.depth_mask = false;
                        pcmd.render_state.blending = blending_state::alpha_blend();
                        pcmd.uniforms = vec![
                            ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                            ("u_scale".into(), UniformValue::Float(vec![1.0])),
                            ("u_color".into(), UniformValue::Float(vec![0.25, 1.0, 0.68, 1.0])),
                            ("u_pointSize".into(), UniformValue::Float(vec![14.0 * self.pixel_ratio])),
                            ("u_round".into(), UniformValue::Float(vec![1.0])),
                        ];
                        pcmd.execute(&self.context, &pass_state).map_err(err)?;
                    }
                }
                if let Some(dva) = &self.ds_lines_va {
                    draw_lines(dva, 1.003, [0.35, 0.95, 1.0, 0.95]).map_err(err)?;
                }
                if let Some(dpva) = &self.ds_points_va {
                    let mut pcmd = DrawCommand::new(dpva, &self.line_program);
                    pcmd.primitive_type = PrimitiveType::Points;
                    pcmd.render_state.depth_test.enabled = true;
                    pcmd.render_state.depth_mask = false;
                    pcmd.render_state.blending = blending_state::alpha_blend();
                    pcmd.uniforms = vec![
                        ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                        ("u_scale".into(), UniformValue::Float(vec![1.004])),
                        ("u_color".into(), UniformValue::Float(vec![0.35, 0.95, 1.0, 1.0])),
                        ("u_pointSize".into(), UniformValue::Float(vec![11.0 * self.pixel_ratio])),
                        ("u_round".into(), UniformValue::Float(vec![1.0])),
                    ];
                    pcmd.execute(&self.context, &pass_state).map_err(err)?;
                }
                for &(pos, col, size) in &self.marker_points {
                    let mva = create_line_va(&self.context, &pos).map_err(err)?;
                    let mut pcmd = DrawCommand::new(&mva, &self.line_program);
                    pcmd.primitive_type = PrimitiveType::Points;
                    pcmd.render_state.depth_test.enabled = true;
                    pcmd.render_state.depth_mask = false;
                    pcmd.render_state.blending = blending_state::alpha_blend();
                    pcmd.uniforms = vec![
                        ("u_viewProjection".into(), mat4_uniform(vp_f32.clone())),
                        ("u_scale".into(), UniformValue::Float(vec![1.003])),
                        ("u_color".into(), UniformValue::Float(col.to_vec())),
                        ("u_pointSize".into(), UniformValue::Float(vec![size * self.pixel_ratio])),
                        ("u_round".into(), UniformValue::Float(vec![1.0])),
                    ];
                    pcmd.execute(&self.context, &pass_state).map_err(err)?;
                }
            }

            // 6. Execute CesiumRS PostProcessStageCollection if active
            if use_pp {
                if let Some(off) = &self.offscreen {
                    let use_hdr = self.pp_tonemapper != "none";
                    self.post_process.update(&self.context, false, use_hdr).map_err(err)?;
                    self.post_process.clear(&self.context).map_err(err)?;
                    self.post_process
                        .execute(&self.context, &off.color, Some(&off.depth), None)
                        .map_err(err)?;
                    let screen_pass = PassState { viewport: Some(vp), framebuffer: None, ..Default::default() };
                    self.post_process.copy(&self.context, None, &screen_pass).map_err(err)?;
                }
            }

            Ok(json!({
                "frame": self.frame_count,
                "tileLevel": self.tile_level,
                "sunDir": [sun_dir.x, sun_dir.y, sun_dir.z],
                "postProcessActive": use_pp,
            })
            .to_string())
        }
    }

    // =========================================================================
    // 2. LIVE glTF 2.0 MODEL VIEWER (Model::from_gltf + CustomShader + Style)
    // =========================================================================

    /// Builds a valid procedural multi-face binary GLB 2.0 (colored octahedron + solar-panel wings)
    /// directly in Rust with zero external asset fetches.
    fn build_procedural_glb() -> Vec<u8> {
        // Octahedron + twin solar panel wings
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut colors: Vec<[f32; 4]> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();

        let mut push_tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3], col: [f32; 4]| {
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
            let n = [n[0] / len, n[1] / len, n[2] / len];
            let base = positions.len() as u16;
            for p in [a, b, c] {
                positions.push(p);
                normals.push(n);
                colors.push(col);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        };

        // Central spacecraft bus (golden-foil octahedron)
        let top = [0.0, 1.15, 0.0];
        let bot = [0.0, -1.15, 0.0];
        let ring = [[0.85, 0.0, 0.0], [0.0, 0.0, 0.85], [-0.85, 0.0, 0.0], [0.0, 0.0, -0.85]];
        let gold = [0.95, 0.72, 0.22, 1.0];
        let silver = [0.80, 0.86, 0.94, 1.0];
        for i in 0..4 {
            let a = ring[i];
            let b = ring[(i + 1) % 4];
            push_tri(top, b, a, if i % 2 == 0 { gold } else { silver });
            push_tri(bot, a, b, if i % 2 == 0 { silver } else { gold });
        }
        // Twin blue solar arrays
        let panel_col = [0.16, 0.45, 0.92, 1.0];
        for side in [-1.0f32, 1.0f32] {
            let x0 = side * 0.95;
            let x1 = side * 2.45;
            let y0 = -0.35;
            let y1 = 0.35;
            let z = 0.04;
            push_tri([x0, y0, z], [x1, y0, z], [x1, y1, z], panel_col);
            push_tri([x0, y0, z], [x1, y1, z], [x0, y1, z], panel_col);
            push_tri([x0, y0, -z], [x1, y1, -z], [x1, y0, -z], panel_col);
            push_tri([x0, y0, -z], [x0, y1, -z], [x1, y1, -z], panel_col);
        }

        let mut min_p = [f32::INFINITY; 3];
        let mut max_p = [f32::NEG_INFINITY; 3];
        for p in &positions {
            for k in 0..3 {
                min_p[k] = min_p[k].min(p[k]);
                max_p[k] = max_p[k].max(p[k]);
            }
        }

        let mut bin = Vec::<u8>::new();
        let pos_off = bin.len();
        for p in &positions {
            for &v in p {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let pos_len = bin.len() - pos_off;

        let norm_off = bin.len();
        for n in &normals {
            for &v in n {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let norm_len = bin.len() - norm_off;

        let col_off = bin.len();
        for c in &colors {
            for &v in c {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
        let col_len = bin.len() - col_off;

        let idx_off = bin.len();
        for &i in &indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let idx_len = bin.len() - idx_off;
        while bin.len() % 4 != 0 {
            bin.push(0);
        }

        let gltf = json!({
            "asset": { "version": "2.0", "generator": "CesiumRS Live Testbed" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "name": "CesiumRS_Orbiter" }],
            "meshes": [{
                "name": "OrbiterMesh",
                "primitives": [{
                    "attributes": { "POSITION": 0, "NORMAL": 1, "COLOR_0": 2 },
                    "indices": 3,
                    "material": 0
                }]
            }],
            "materials": [{
                "name": "OrbiterPBR",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor": 0.35,
                    "roughnessFactor": 0.45
                },
                "doubleSided": true
            }],
            "buffers": [{ "byteLength": bin.len() }],
            "bufferViews": [
                { "buffer": 0, "byteOffset": pos_off, "byteLength": pos_len, "target": 34962 },
                { "buffer": 0, "byteOffset": norm_off, "byteLength": norm_len, "target": 34962 },
                { "buffer": 0, "byteOffset": col_off, "byteLength": col_len, "target": 34962 },
                { "buffer": 0, "byteOffset": idx_off, "byteLength": idx_len, "target": 34963 }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": positions.len(), "type": "VEC3", "min": min_p, "max": max_p },
                { "bufferView": 1, "componentType": 5126, "count": normals.len(), "type": "VEC3" },
                { "bufferView": 2, "componentType": 5126, "count": colors.len(), "type": "VEC4" },
                { "bufferView": 3, "componentType": 5123, "count": indices.len(), "type": "SCALAR" }
            ]
        });
        let mut json_bytes = serde_json::to_vec(&gltf).unwrap();
        while json_bytes.len() % 4 != 0 {
            json_bytes.push(b' ');
        }

        let total_len = 12 + 8 + json_bytes.len() + 8 + bin.len();
        let mut glb = Vec::with_capacity(total_len);
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_len as u32).to_le_bytes());
        glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json_bytes);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"BIN\0");
        glb.extend_from_slice(&bin);
        glb
    }

    #[wasm_bindgen]
    pub struct LiveModelViewer {
        context: Context<BrowserGl>,
        model: Option<Model<BrowserGl>>,
        glb_bytes: Vec<u8>,
        width: u32,
        height: u32,
        frame_num: u32,
        yaw: f64,
        pitch: f64,
        distance: f64,
    }

    #[wasm_bindgen]
    impl LiveModelViewer {
        #[wasm_bindgen(constructor)]
        pub fn new(gl: JsValue, width: u32, height: u32) -> Result<LiveModelViewer, JsValue> {
            let context = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
            Ok(LiveModelViewer {
                context,
                model: None,
                glb_bytes: build_procedural_glb(),
                width: width.max(1),
                height: height.max(1),
                frame_num: 1,
                yaw: 0.6,
                pitch: 0.35,
                distance: 5.5,
            })
        }

        #[wasm_bindgen(js_name = loadModel)]
        pub async fn load_model(&mut self, custom_glb: Option<Vec<u8>>, config_json: &str) -> Result<String, JsValue> {
            if let Some(bytes) = custom_glb {
                if !bytes.is_empty() {
                    self.glb_bytes = bytes;
                }
            }
            let cfg: Value = serde_json::from_str(config_json).unwrap_or(json!({}));
            let cache = ResourceCache::new(ResourceClient::new(Rc::new(BrowserTransport::default())));
            let host = Rc::new(BrowserImageHost);
            let resource = Resource::new("https://testbed.local/model.glb").ok_or_else(|| err("invalid url"))?;
            let mut options = ModelOptions::default();
            options.id = Some("live-gltf-model".into());
            options.cull = false;
            if let Some(s) = cfg["scale"].as_f64() {
                options.scale = s;
            }
            if let Some(a) = cfg["color"].as_array() {
                if a.len() == 4 {
                    options.color = Some(Color::new(
                        a[0].as_f64().unwrap_or(1.0),
                        a[1].as_f64().unwrap_or(1.0),
                        a[2].as_f64().unwrap_or(1.0),
                        a[3].as_f64().unwrap_or(1.0),
                    ));
                }
            }
            options.color_blend_mode = match cfg["colorBlendMode"].as_str().unwrap_or("highlight") {
                "replace" => ColorBlendMode::Replace,
                "mix" => ColorBlendMode::Mix,
                _ => ColorBlendMode::Highlight,
            };
            if let Some(amt) = cfg["colorBlendAmount"].as_f64() {
                options.color_blend_amount = amt;
            }

            let mut model = Model::from_gltf(
                cache,
                resource,
                None,
                Some(GltfJsonInput::Bytes(BufferSource::new(self.glb_bytes.clone()))),
                self.context.clone(),
                host,
                options,
            )
            .await
            .map_err(err)?;

            if let Some(fs_text) = cfg["customShaderFs"].as_str().filter(|s| !s.trim().is_empty()) {
                let lighting = match cfg["lightingModel"].as_str().unwrap_or("pbr") {
                    "unlit" => Some(LightingModel::Unlit),
                    _ => Some(LightingModel::Pbr),
                };
                let shader = CustomShader::new(CustomShaderOptions {
                    mode: CustomShaderMode::ModifyMaterial,
                    lighting_model: lighting,
                    translucency_mode: CustomShaderTranslucencyMode::Inherit,
                    uniforms: Default::default(),
                    varyings: Default::default(),
                    vertex_shader_text: None,
                    fragment_shader_text: Some(fs_text.to_owned()),
                })
                .map_err(err)?;
                model.custom_shader = Some(Rc::new(shader));
            }
            if let Some(style_val) = cfg.get("style").filter(|v| !v.is_null()) {
                let style = Cesium3DTileStyle::new(Some(style_val)).map_err(err)?;
                model.set_style(Some(Rc::new(RefCell::new(style))));
            }

            // Pump frames until model is ready
            for _ in 0..8 {
                let mut frame = self.make_frame();
                model.update(&mut frame).map_err(err)?;
                for cb in std::mem::take(&mut frame.after_render) {
                    cb().map_err(err)?;
                }
                if model.ready() {
                    break;
                }
            }
            let ready = model.ready();
            let bs = model.bounding_sphere().ok();
            self.model = Some(model);
            Ok(json!({
                "ready": ready,
                "glbBytes": self.glb_bytes.len(),
                "boundingSphere": bs.map(|b| json!({"center": [b.center.x, b.center.y, b.center.z], "radius": b.radius})),
            })
            .to_string())
        }

        #[wasm_bindgen(js_name = setOrbit)]
        pub fn set_orbit(&mut self, yaw: f64, pitch: f64, distance: f64) {
            self.yaw = yaw;
            self.pitch = pitch.clamp(-1.45, 1.45);
            self.distance = distance.clamp(1.2, 40.0);
        }

        pub fn resize(&mut self, width: u32, height: u32) {
            self.width = width.max(1);
            self.height = height.max(1);
        }

        fn make_frame(&mut self) -> FrameState {
            let mut camera = Camera::new(CameraOptions::new(self.width as f64, self.height as f64));
            let cp = self.pitch.cos();
            let eye = Cartesian3::new(
                self.distance * cp * self.yaw.sin(),
                self.distance * self.pitch.sin(),
                self.distance * cp * self.yaw.cos(),
            );
            let dir = (Cartesian3::ZERO - eye).normalize();
            let right = dir.cross(Cartesian3::UNIT_Y).normalize();
            let up = right.cross(dir).normalize();
            camera.position = eye;
            camera.direction = dir;
            camera.right = right;
            camera.up = up;
            camera.frustum = CameraFrustum::Perspective(PerspectiveFrustum {
                fov: FRAC_PI_3,
                aspect_ratio: self.width as f64 / self.height.max(1) as f64,
                near: 0.1,
                far: 200.0,
                x_offset: 0.0,
                y_offset: 0.0,
            });
            let snapshot = camera.uniform_camera();
            let culling = camera
                .frustum
                .compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
            let mut frame = FrameState::new(
                UniformFrame::new(snapshot, JulianDate::from_date(js_sys::Date::now())),
                culling,
                [self.width, self.height],
            );
            frame.frame_number = self.frame_num;
            self.frame_num = self.frame_num.wrapping_add(1);
            frame.passes.render = true;
            frame.stencil_buffer = true;
            frame
        }

        pub fn render(&mut self) -> Result<usize, JsValue> {
            let mut frame = self.make_frame();
            if let Some(model) = &mut self.model {
                model.update(&mut frame).map_err(err)?;
            }
            self.context.uniform_state().update(&frame.uniform, self.width).map_err(err)?;
            let vp = Viewport { x: 0, y: 0, width: self.width, height: self.height };
            let pass_state = PassState { viewport: Some(vp), ..Default::default() };
            ClearCommand {
                color: Some([0.05, 0.07, 0.11, 1.0]),
                depth: Some(1.0),
                stencil: Some(0),
                ..Default::default()
            }
            .execute(&self.context, &pass_state)
            .map_err(err)?;
            let count = frame.command_list.len();
            for cmd in std::mem::take(&mut frame.command_list) {
                let submission = SceneSubmission {
                    pass: cmd.pass,
                    frustum: 0,
                    near: 0.1,
                    far: 200.0,
                    draw_mode: SceneDrawMode::default(),
                    oit: None,
                    metadata_pick: None,
                    framebuffer: None,
                    viewport: Some(vp),
                    scissor_test: None,
                    blending_enabled: None,
                    metadata: cmd.metadata.clone(),
                    clipping_planes: None,
                    tileset_draw: None,
                };
                (cmd.execute)(submission).map_err(err)?;
            }
            for cb in std::mem::take(&mut frame.after_render) {
                cb().map_err(err)?;
            }
            Ok(count)
        }
    }

    // =========================================================================
    // 3. GPU SCENE EFFECTS PREVIEWS (Sun, BRDF LUT, AutoExposure, 3D Tile Style)
    // =========================================================================

    #[wasm_bindgen]
    pub fn render_sun_preview(gl: JsValue, glow_factor: f64, size: u32) -> Result<String, JsValue> {
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let mut camera = Camera::new(CameraOptions::new(f64::from(size), f64::from(size)));
        let uniform = UniformFrame::new(camera.uniform_camera(), JulianDate::from_date(js_sys::Date::now()));
        let mut frame = FrameState::new(uniform, CullingVolume::new(Vec::new()), [size, size]);
        frame.passes.render = true;
        let mut sun = Sun::new(context.clone());
        sun.set_glow_factor(glow_factor);
        let vp = Viewport { x: 0, y: 0, width: size, height: size };
        let mut cmds = sun.update(&frame, vp, false).map_err(err)?.ok_or_else(|| err("Sun produced no command"))?;
        let mut engine = ComputeEngine::new(&context);
        if let Some(compute) = cmds.compute_command.as_mut() {
            compute.execute(&mut engine).map_err(err)?;
        }
        let tex = sun.texture().ok_or_else(|| err("No sun texture"))?.clone();
        // Blit generated sun texture directly to screen
        let quad = ViewportQuad::new(&context).map_err(err)?;
        let prog = ShaderProgram::from_sources(
            &context,
            &ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(
                "in vec2 v_textureCoordinates; uniform sampler2D u_tex; void main() { out_FragColor = texture(u_tex, v_textureCoordinates); }",
            )),
        )
        .map_err(err)?;
        ClearCommand { color: Some([0.02, 0.03, 0.06, 1.0]), ..Default::default() }
            .execute(&context, &PassState { viewport: Some(vp), ..Default::default() })
            .map_err(err)?;
        let mut draw = DrawCommand::new(quad.vertex_array(), &prog);
        draw.textures = vec![("u_tex".into(), &*tex)];
        draw.execute(&context, &PassState { viewport: Some(vp), ..Default::default() }).map_err(err)?;
        Ok(json!({"textureWidth": tex.width(), "textureHeight": tex.height(), "glowFactor": glow_factor}).to_string())
    }

    #[wasm_bindgen]
    pub fn render_brdf_lut_preview(gl: JsValue, size: u32) -> Result<String, JsValue> {
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let mut gen = BrdfLutGenerator::new();
        gen.update(&context).map_err(err)?;
        let tex = gen.color_texture().ok_or_else(|| err("No BRDF LUT texture"))?.clone();
        let vp = Viewport { x: 0, y: 0, width: size, height: size };
        let quad = ViewportQuad::new(&context).map_err(err)?;
        let prog = ShaderProgram::from_sources(
            &context,
            &ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(
                "in vec2 v_textureCoordinates; uniform sampler2D u_tex; void main() { vec4 c = texture(u_tex, v_textureCoordinates); out_FragColor = vec4(c.rg, 0.25, 1.0); }",
            )),
        )
        .map_err(err)?;
        let mut draw = DrawCommand::new(quad.vertex_array(), &prog);
        draw.textures = vec![("u_tex".into(), &*tex)];
        draw.execute(&context, &PassState { viewport: Some(vp), ..Default::default() }).map_err(err)?;
        Ok(json!({"lutWidth": tex.width(), "lutHeight": tex.height()}).to_string())
    }

    #[wasm_bindgen]
    pub fn run_auto_exposure_preview(gl: JsValue, r: u8, g: u8, b: u8) -> Result<String, JsValue> {
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let color = Rc::new(Texture::new(&context, TextureOptions::rgba(16, 16), Some(&[r, g, b, 255].repeat(256))).map_err(err)?);
        let mut exposure = AutoExposure::new();
        let mut history = Vec::new();
        for _ in 0..4 {
            exposure.update(&context).map_err(err)?;
            exposure.clear(&context).map_err(err)?;
            exposure.execute(&context, &color).map_err(err)?;
            let fb = Framebuffer::new(
                &context,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(exposure.output_texture().unwrap())],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(err)?;
            let px = context
                .read_pixels_from(ReadState { region: TextureRegion { x: 0, y: 0, width: 1, height: 1 }, framebuffer: Some(&fb) })
                .map_err(err)?;
            let vals: Vec<f32> = px.bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            history.push(vals);
        }
        Ok(json!({"inputRgb": [r, g, b], "iterations": history}).to_string())
    }

    struct JsonStyleFeature<'a>(&'a Value);
    impl<'a> StyleFeature for JsonStyleFeature<'a> {
        fn property_inherited(&self, name: &str) -> Option<StyleValue> {
            self.0.get(name).map(StyleValue::from_json)
        }
        fn is_exact_class(&self, class_name: &str) -> bool {
            self.0.get("class").and_then(|v| v.as_str()) == Some(class_name)
        }
        fn is_class(&self, class_name: &str) -> bool {
            self.is_exact_class(class_name)
        }
        fn exact_class_name(&self) -> Option<String> {
            self.0.get("class").and_then(|v| v.as_str()).map(String::from)
        }
        fn tileset_time(&self) -> f64 {
            12.0
        }
    }

    #[wasm_bindgen]
    pub fn evaluate_tile_style(style_json: &str, features_json: &str) -> Result<String, JsValue> {
        let style_val: Value = serde_json::from_str(style_json).map_err(err)?;
        let features: Vec<Value> = serde_json::from_str(features_json).map_err(err)?;
        let style = Cesium3DTileStyle::new(Some(&style_val)).map_err(err)?;
        let mut results = Vec::new();
        for feat_val in &features {
            let f = JsonStyleFeature(feat_val);
            let show = match style.show() {
                Some(expr) => match expr.evaluate(Some(&f)).map_err(err)? {
                    StyleValue::Boolean(b) => b,
                    _ => true,
                },
                None => true,
            };
            let color = match style.color() {
                Some(expr) => expr
                    .evaluate_color(Some(&f))
                    .map_err(err)?
                    .map(|c| [c.red, c.green, c.blue, c.alpha]),
                None => None,
            };
            let point_size = match style.point_size() {
                Some(expr) => Some(expr.evaluate(Some(&f)).map_err(err)?.to_number()),
                None => None,
            };
            results.push(json!({
                "feature": feat_val,
                "show": show,
                "color": color,
                "pointSize": point_size,
            }));
        }
        let mut var_map = BTreeMap::new();
        for k in ["height", "Height", "area", "Area", "yearBuilt", "YearBuilt", "temperature", "name"] {
            var_map.insert(k.to_string(), format!("v_{k}"));
        }
        let mut shader_state = ShaderState::default();
        let color_glsl = style
            .color()
            .map(|e| e.get_shader_function("vec4 getColor()", &var_map, &mut shader_state, "vec4").map_err(|e| e.message));
        Ok(json!({
            "results": results,
            "colorGlsl": match color_glsl {
                Some(Ok(s)) => json!(s),
                Some(Err(e)) => json!({"error": e}),
                None => Value::Null,
            },
            "translucent": shader_state.translucent,
        })
        .to_string())
    }

    // =========================================================================
    // 4. DATASOURCES LAB (CZML, GeoJSON, KML, GPX, exportKml, Time Properties)
    // =========================================================================

    fn extract_entities_summary_and_overlay(entities: &EntityCollection, time: &JulianDate) -> Value {
        let mut list = Vec::new();
        let mut overlay_points = Vec::new();
        let mut overlay_polylines = Vec::new();

        for entity in entities.values() {
            let pos = entity
                .position()
                .and_then(|p| p.get_value(Some(time), None).ok().flatten());
            if let Some(p) = pos {
                overlay_points.push(json!([p.x, p.y, p.z]));
            }
            // Extract polyline positions if present
            if let Some(PropertyInput::Component(comp)) = entity.get_property("polyline") {
                if let Some(pl) = comp.as_any().downcast_ref::<PolylineGraphics>() {
                    if let Some(PropertyInput::Component(c)) = pl.get("positions") {
                        if let Ok(Some(DynamicValue::Positions(arr))) = c.get_value(time, None) {
                            overlay_polylines.push(Value::Array(arr.iter().map(|p| json!([p.x, p.y, p.z])).collect()));
                        }
                    } else if let Some(PropertyInput::Value(DynamicValue::Positions(arr))) = pl.get("positions") {
                        overlay_polylines.push(Value::Array(arr.iter().map(|p| json!([p.x, p.y, p.z])).collect()));
                    }
                }
            }
            // Extract polygon hierarchy outer ring if present
            if let Some(PropertyInput::Component(comp)) = entity.get_property("polygon") {
                if let Some(pg) = comp.as_any().downcast_ref::<PolygonGraphics>() {
                    let h_val = match pg.get("hierarchy") {
                        Some(PropertyInput::Component(c)) => c.get_value(time, None).ok().flatten(),
                        Some(PropertyInput::Value(v)) => Some(v),
                        None => None,
                    };
                    if let Some(DynamicValue::PolygonHierarchy(h)) = h_val {
                        let mut ring: Vec<Value> = h.positions.iter().map(|p| json!([p.x, p.y, p.z])).collect();
                        if let Some(first) = ring.first().cloned() {
                            ring.push(first);
                        }
                        overlay_polylines.push(Value::Array(ring));
                    }
                }
            }
            let carto = pos.and_then(|p| Ellipsoid::WGS84.cartesian_to_cartographic(p));
            list.push(json!({
                "id": entity.id(),
                "name": entity.name(),
                "hasPosition": pos.is_some(),
                "lonLatAlt": carto.map(|c| [to_degrees(c.longitude), to_degrees(c.latitude), c.height]),
                "hasPoint": entity.get_property("point").is_some(),
                "hasBillboard": entity.get_property("billboard").is_some(),
                "hasLabel": entity.get_property("label").is_some(),
                "hasPolyline": entity.get_property("polyline").is_some(),
                "hasPolygon": entity.get_property("polygon").is_some(),
                "hasPath": entity.get_property("path").is_some(),
            }));
        }
        json!({
            "entityCount": list.len(),
            "entities": list,
            "overlay": {
                "points": overlay_points,
                "polylines": overlay_polylines,
            }
        })
    }

    #[wasm_bindgen]
    pub async fn parse_czml(text: &str, sample_iso: Option<String>) -> Result<String, JsValue> {
        let ds = CzmlDataSource::load_new(CzmlInput::Text(text.to_owned()), &CzmlLoadOptions::default())
            .await
            .map_err(err)?;
        let clock = ds.clock();
        let eval_time = sample_iso
            .as_deref()
            .and_then(|s| JulianDate::from_iso8601(s).ok())
            .or_else(|| clock.as_ref().and_then(|c| c.current_time()).map(|r| *r))
            .or_else(|| clock.as_ref().and_then(|c| c.start_time()).map(|r| *r))
            .unwrap_or_else(|| JulianDate::from_date(js_sys::Date::now()));

        let mut summary = extract_entities_summary_and_overlay(&ds.entities(), &eval_time);

        // Also sample trajectories of time-dynamic entities for globe visualization
        if let (Some(start), Some(stop)) = (
            clock.as_ref().and_then(|c| c.start_time()),
            clock.as_ref().and_then(|c| c.stop_time()),
        ) {
            let dur = JulianDate::seconds_difference(&stop, &start);
            if dur > 0.0 {
                let mut tracks = Vec::new();
                for entity in ds.entities().values() {
                    if let Some(pos_prop) = entity.position() {
                        if !pos_prop.is_constant() {
                            let mut pts = Vec::new();
                            for i in 0..=48 {
                                let t = start.add_seconds(dur * i as f64 / 48.0);
                                if let Ok(Some(p)) = pos_prop.get_value(Some(&t), None) {
                                    pts.push(json!([p.x, p.y, p.z]));
                                }
                            }
                            if pts.len() >= 2 {
                                tracks.push(Value::Array(pts));
                            }
                        }
                    }
                }
                if let Some(arr) = summary["overlay"]["polylines"].as_array_mut() {
                    arr.extend(tracks);
                }
            }
        }

        Ok(json!({
            "format": "CZML",
            "name": ds.name(),
            "version": ds.version(),
            "clock": clock.map(|c| json!({
                "start": c.start_time().map(|t| t.to_iso8601(None)),
                "stop": c.stop_time().map(|t| t.to_iso8601(None)),
                "current": c.current_time().map(|t| t.to_iso8601(None)),
                "multiplier": c.multiplier(),
            })),
            "summary": summary,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub async fn parse_geojson(text: &str) -> Result<String, JsValue> {
        let ds = GeoJsonDataSource::new(Some("live-geojson".into()));
        ds.load(text, &GeoJsonOptions::default()).await.map_err(err)?;
        let now = JulianDate::from_date(js_sys::Date::now());
        let summary = extract_entities_summary_and_overlay(&ds.entities(), &now);
        Ok(json!({
            "format": "GeoJSON",
            "name": ds.name(),
            "summary": summary,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub async fn parse_kml(text: &str) -> Result<String, JsValue> {
        let ds = KmlDataSource::load_new(KmlInput::Xml(text.to_owned()), KmlOptions::default())
            .await
            .map_err(err)?;
        let now = JulianDate::from_date(js_sys::Date::now());
        let summary = extract_entities_summary_and_overlay(&ds.entities(), &now);
        Ok(json!({
            "format": "KML",
            "name": ds.name(),
            "tours": ds.kml_tours().len(),
            "summary": summary,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub async fn parse_gpx(text: &str) -> Result<String, JsValue> {
        let ds = GpxDataSource::load_new(GpxInput::Xml(text.to_owned()), GpxOptions::default())
            .await
            .map_err(err)?;
        let now = JulianDate::from_date(js_sys::Date::now());
        let summary = extract_entities_summary_and_overlay(&ds.entities(), &now);
        Ok(json!({
            "format": "GPX",
            "name": ds.name(),
            "version": ds.version(),
            "creator": ds.creator(),
            "summary": summary,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub async fn export_kml_from_geojson(geojson_text: &str, as_kmz: bool) -> Result<String, JsValue> {
        let ds = GeoJsonDataSource::new(Some("exported".into()));
        ds.load(geojson_text, &GeoJsonOptions::default()).await.map_err(err)?;
        let mut opts = ExportKmlOptions::new(ds.entities().clone());
        opts.kmz = as_kmz;
        match export_kml(opts).map_err(err)? {
            ExportKmlResult::Kml { kml, external_files } => Ok(json!({
                "mode": "kml",
                "kml": kml,
                "externalFiles": external_files.len(),
            })
            .to_string()),
            ExportKmlResult::Kmz(bytes) => Ok(json!({
                "mode": "kmz",
                "byteLength": bytes.len(),
                "zipSignature": format!("{:02X}{:02X}", bytes.first().copied().unwrap_or(0), bytes.get(1).copied().unwrap_or(0)),
            })
            .to_string()),
        }
    }

    #[wasm_bindgen]
    pub fn eval_time_properties(elapsed_sec: f64) -> Result<String, JsValue> {
        let epoch = JulianDate::from_iso8601("2026-06-21T12:00:00Z").unwrap();
        let t = epoch.add_seconds(elapsed_sec);

        // SampledProperty with Linear, Lagrange, and Hermite interpolation
        let lin = SampledProperty::<f64>::new(None);
        let lag = SampledProperty::<f64>::new(None);
        lag.set_interpolation_options(Some(Interpolation::Lagrange), Some(2.0));
        let her = SampledProperty::<f64>::new(None);
        her.set_interpolation_options(Some(Interpolation::Hermite), Some(2.0));

        let times = [epoch, epoch.add_seconds(30.0), epoch.add_seconds(60.0), epoch.add_seconds(100.0)];
        let vals = [100.0, 420.0, 260.0, 680.0];
        lin.add_samples(&times, &vals, None).map_err(err)?;
        lag.add_samples(&times, &vals, None).map_err(err)?;
        her.add_samples(&times, &vals, None).map_err(err)?;

        // TimeIntervalCollectionProperty
        let intervals = TimeIntervalCollectionProperty::new();
        intervals.intervals().add_interval(TimeInterval::new(epoch, epoch.add_seconds(50.0), true, false, Some(Rc::new("ASCENT".to_string()))));
        intervals.intervals().add_interval(TimeInterval::new(epoch.add_seconds(50.0), epoch.add_seconds(100.0), true, true, Some(Rc::new("CRUISE".to_string()))));

        // SampledPositionProperty + VelocityVectorProperty + VelocityOrientationProperty
        let pos = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
        let p0 = Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(-105.0, 40.0, 1000.0));
        let p1 = Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(-104.5, 40.3, 5000.0));
        let p2 = Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(-104.0, 40.2, 8000.0));
        pos.add_samples(&[epoch, epoch.add_seconds(50.0), epoch.add_seconds(100.0)], &[p0, p1, p2], None).map_err(err)?;
        let vel = VelocityVectorProperty::new(Some(pos.clone()), false);
        let ori = VelocityOrientationProperty::new(Some(pos.clone()), None);

        let cur_pos = pos.get_value(Some(&t), None).map_err(err)?;
        let cur_vel = vel.get_value(Some(&t), None).map_err(err)?;
        let cur_ori = ori.get_value(Some(&t), None).map_err(err)?;

        // Clock tick test using Clock::new_at (safe on wasm32!)
        let mut clock = Clock::new_at(
            ClockOptions {
                start_time: Some(epoch),
                stop_time: Some(epoch.add_seconds(100.0)),
                current_time: Some(epoch),
                multiplier: Some(10.0),
                should_animate: Some(true),
                clock_range: Some(ClockRange::LoopStop),
                ..Default::default()
            },
            0.0,
        )
        .unwrap();
        let ticked = clock.tick_at(elapsed_sec * 100.0);

        Ok(json!({
            "timeIso": t.to_iso8601(None),
            "linear": lin.get_value(Some(&t), None).map_err(err)?,
            "lagrange": lag.get_value(Some(&t), None).map_err(err)?,
            "hermite": her.get_value(Some(&t), None).map_err(err)?,
            "phaseInterval": intervals.get_value(Some(&t), None).map_err(err)?,
            "position": cur_pos.map(|p| [p.x, p.y, p.z]),
            "speedMps": cur_vel.map(|v| v.magnitude()),
            "orientationQuat": cur_ori.map(|q| [q.x, q.y, q.z, q.w]),
            "clockTickedIso": ticked.to_iso8601(None),
        })
        .to_string())
    }

    // =========================================================================
    // 5. CORE MATH, GEODESY, SPATIAL INDICES & ASTRONOMY
    // =========================================================================

    #[wasm_bindgen]
    pub fn core_geodesy_compare(body: &str, lon1_deg: f64, lat1_deg: f64, lon2_deg: f64, lat2_deg: f64) -> Result<String, JsValue> {
        let e = ellipsoid_by_name(body);
        let c1 = Cartographic::from_degrees(lon1_deg, lat1_deg, 0.0);
        let c2 = Cartographic::from_degrees(lon2_deg, lat2_deg, 0.0);
        let p1 = e.cartographic_to_cartesian(&c1);
        let p2 = e.cartographic_to_cartesian(&c2);
        let geo = EllipsoidGeodesic::new(&c1, &c2, &e).ok_or_else(|| err("Antipodal or degenerate geodesic"))?;
        let rhumb = EllipsoidRhumbLine::new(&c1, &c2, &e);
        let mid_g = geo.interpolate_using_fraction(0.5);
        let mid_r = rhumb.interpolate_using_fraction(0.5);
        let geo_proj = GeographicProjection::new(e).project(&c1);
        let wm_proj = WebMercatorProjection::new(e).project(&c1);
        let enu = Transforms::east_north_up_to_fixed_frame(p1, &e);

        Ok(json!({
            "body": body,
            "radii": [e.radii.x, e.radii.y, e.radii.z],
            "p1Ecef": [p1.x, p1.y, p1.z],
            "p2Ecef": [p2.x, p2.y, p2.z],
            "chordKm": (p2 - p1).magnitude() / 1000.0,
            "geodesic": {
                "distanceKm": geo.surface_distance / 1000.0,
                "startHeadingDeg": to_degrees(geo.start_heading),
                "endHeadingDeg": to_degrees(geo.end_heading),
                "midpointDeg": [to_degrees(mid_g.longitude), to_degrees(mid_g.latitude)],
            },
            "rhumb": {
                "distanceKm": rhumb.surface_distance / 1000.0,
                "headingDeg": to_degrees(rhumb.heading),
                "midpointDeg": [to_degrees(mid_r.longitude), to_degrees(mid_r.latitude)],
            },
            "projectionsAtStart": {
                "geographicMeters": [geo_proj.x, geo_proj.y, geo_proj.z],
                "webMercatorMeters": [wm_proj.x, wm_proj.y, wm_proj.z],
            },
            "enuFrameCols": [
                [enu.0[0], enu.0[1], enu.0[2]],
                [enu.0[4], enu.0[5], enu.0[6]],
                [enu.0[8], enu.0[9], enu.0[10]],
            ],
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub fn core_spatial_inspect(lon_deg: f64, lat_deg: f64, level: u32, mx: u32, my: u32, mz: u32) -> Result<String, JsValue> {
        let e = Ellipsoid::WGS84;
        let c = Cartographic::from_degrees(lon_deg, lat_deg, 0.0);
        let lvl = level.clamp(1, 20);
        let geo_scheme = GeographicTilingScheme::new(e, Rectangle::MAX_VALUE, 2, 1);
        let wm_scheme = WebMercatorTilingScheme::new(e, 1, 1, None);
        let geo_xy = geo_scheme.position_to_tile_xy(&c, lvl);
        let wm_xy = wm_scheme.position_to_tile_xy(&c, lvl);

        // S2 cell derived from cube face projection
        let u_pos = e.cartographic_to_cartesian(&c).normalize();
        let (ax, ay, az) = (u_pos.x.abs(), u_pos.y.abs(), u_pos.z.abs());
        let face: u8 = if ax >= ay && ax >= az {
            if u_pos.x >= 0.0 { 0 } else { 3 }
        } else if ay >= az {
            if u_pos.y >= 0.0 { 1 } else { 4 }
        } else if u_pos.z >= 0.0 {
            2
        } else {
            5
        };
        let s2_lvl = lvl.min(12);
        let h_dim = 1u64 << s2_lvl;
        let s2_x = (((lon_deg + 180.0) / 360.0).clamp(0.0, 0.9999) * h_dim as f64) as u64;
        let s2_y = (((lat_deg + 90.0) / 180.0).clamp(0.0, 0.9999) * h_dim as f64) as u64;
        let h_idx = HilbertOrder::encode_2d(s2_lvl, s2_x, s2_y).unwrap_or(0);
        let h_dec = HilbertOrder::decode_2d(s2_lvl, h_idx);
        let s2 = S2Cell::from_face_position_level(face, h_idx as u64, s2_lvl);
        let s2_info = s2.map(|cell| {
            let center = cell.get_center(&e);
            let cc = e.cartesian_to_cartographic(center);
            let verts: Vec<[f64; 2]> = (0..4)
                .filter_map(|i| cell.get_vertex(i, &e).and_then(|v| e.cartesian_to_cartographic(v)))
                .map(|vc| [to_degrees(vc.longitude), to_degrees(vc.latitude)])
                .collect();
            json!({
                "cellIdHex": format!("0x{:016x}", cell.cell_id()),
                "token": S2Cell::get_token_from_id(cell.cell_id()),
                "face": face,
                "level": cell.level(),
                "parentToken": cell.get_parent().map(|p| S2Cell::get_token_from_id(p.cell_id())),
                "childrenTokens": (0..4).filter_map(|i| cell.get_child(i).map(|ch| S2Cell::get_token_from_id(ch.cell_id()))).collect::<Vec<_>>(),
                "centerDeg": cc.map(|v| [to_degrees(v.longitude), to_degrees(v.latitude)]),
                "verticesDeg": verts,
            })
        });

        let m2 = MortonOrder::encode_2d(mx.min(65535), my.min(65535));
        let m2_dec = m2.map(MortonOrder::decode_2d);
        let m3 = MortonOrder::encode_3d(mx.min(1023), my.min(1023), mz.min(1023));
        let m3_dec = m3.and_then(MortonOrder::decode_3d);

        Ok(json!({
            "geographicTile": geo_xy.map(|v| [v.x as u64, v.y as u64]),
            "webMercatorTile": wm_xy.map(|v| [v.x as u64, v.y as u64]),
            "morton2d": { "encoded": m2, "decoded": m2_dec },
            "morton3d": { "encoded": m3, "decoded": m3_dec },
            "hilbert2d": { "level": s2_lvl, "xy": [s2_x, s2_y], "encoded": h_idx.to_string(), "decoded": h_dec },
            "s2Cell": s2_info,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub fn core_astronomy_inspect(iso_or_ms: &str) -> Result<String, JsValue> {
        let jd = if let Ok(parsed) = JulianDate::from_iso8601(iso_or_ms) {
            parsed
        } else if let Ok(ms) = iso_or_ms.parse::<f64>() {
            JulianDate::from_date(ms)
        } else {
            JulianDate::from_date(js_sys::Date::now())
        };
        let greg = jd.to_gregorian_date();
        let sun_icrf = compute_sun_position_in_earth_inertial_frame(&jd);
        let moon_icrf = compute_moon_position_in_earth_inertial_frame(&jd);
        let rot = Transforms::compute_teme_to_pseudo_fixed_matrix(&jd);
        let sun_ecef = rot.multiply_by_vector(sun_icrf);
        let moon_ecef = rot.multiply_by_vector(moon_icrf);
        let e = Ellipsoid::WGS84;
        let subsolar = e.cartesian_to_cartographic(sun_ecef.normalize().multiply_by_scalar(e.maximum_radius));
        let sublunar = e.cartesian_to_cartographic(moon_ecef.normalize().multiply_by_scalar(e.maximum_radius));
        let tai_minus_utc = jd.compute_tai_minus_utc();

        Ok(json!({
            "iso8601": jd.to_iso8601(Some(3)),
            "julianDay": jd.total_days(),
            "dayNumber": jd.day_number,
            "secondsOfDay": jd.seconds_of_day,
            "taiMinusUtcSec": tai_minus_utc,
            "gregorian": {
                "year": greg.year, "month": greg.month, "day": greg.day,
                "hour": greg.hour, "minute": greg.minute, "second": greg.second
            },
            "sun": {
                "icrfKm": [sun_icrf.x / 1000.0, sun_icrf.y / 1000.0, sun_icrf.z / 1000.0],
                "distanceAu": sun_icrf.magnitude() / 1.4959787e11,
                "subsolarDeg": subsolar.map(|c| [to_degrees(c.longitude), to_degrees(c.latitude)]),
            },
            "moon": {
                "icrfKm": [moon_icrf.x / 1000.0, moon_icrf.y / 1000.0, moon_icrf.z / 1000.0],
                "distanceKm": moon_icrf.magnitude() / 1000.0,
                "sublunarDeg": sublunar.map(|c| [to_degrees(c.longitude), to_degrees(c.latitude)]),
            },
        })
        .to_string())
    }

    // =========================================================================
    // 6. WORKERS: 31 GEOMETRY WORKERS, TERRAIN MESHER, GAUSSIAN SPLATS, CODECS
    // =========================================================================

    #[wasm_bindgen]
    pub fn worker_geometry_list() -> String {
        json!(GeometryDescription::MODULE_NAMES).to_string()
    }

    #[wasm_bindgen]
    pub fn worker_geometry_build(module_name: &str) -> Result<String, JsValue> {
        let t0 = js_sys::Date::now();
        let e = Ellipsoid::UNIT_SPHERE;
        let surface_ring = |r_deg: f64| -> Vec<Cartesian3> {
            [(-r_deg, -r_deg), (r_deg, -r_deg), (r_deg, r_deg), (-r_deg, r_deg)]
                .iter()
                .map(|&(lon, lat)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
                .collect()
        };
        let path_pts: Vec<Cartesian3> = [(-25.0, -10.0), (0.0, 12.0), (25.0, -6.0)]
            .iter()
            .map(|&(lon, lat)| e.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
            .collect();
        let center = e.cartographic_to_cartesian(&Cartographic::from_degrees(0.0, 0.0, 0.0));

        let desc: GeometryDescription = match module_name {
            "createBoxGeometry" => GeometryDescription::BoxGeometry(
                BoxGeometry::from_dimensions(Cartesian3::new(1.4, 1.0, 0.8), VertexFormat::POSITION_AND_NORMAL, None).unwrap(),
            ),
            "createBoxOutlineGeometry" => GeometryDescription::BoxOutlineGeometry(
                BoxOutlineGeometry::from_dimensions(Cartesian3::new(1.4, 1.0, 0.8), None).unwrap(),
            ),
            "createSphereGeometry" => GeometryDescription::SphereGeometry(
                SphereGeometry::new(0.95, 24.0, 16.0, VertexFormat::POSITION_AND_NORMAL).unwrap(),
            ),
            "createSphereOutlineGeometry" => GeometryDescription::SphereOutlineGeometry(
                SphereOutlineGeometry::new(0.95, 16.0, 12.0, 12.0).unwrap(),
            ),
            "createEllipsoidGeometry" => GeometryDescription::EllipsoidGeometry(
                EllipsoidGeometry::new(Cartesian3::new(1.2, 0.85, 0.65), None, 0.0, 2.0 * PI, 0.0, PI, 24.0, 16.0, VertexFormat::POSITION_AND_NORMAL, None)
                    .ok_or_else(|| err("Invalid EllipsoidGeometry"))?,
            ),
            "createEllipsoidOutlineGeometry" => GeometryDescription::EllipsoidOutlineGeometry(
                EllipsoidOutlineGeometry::new(Cartesian3::new(1.2, 0.85, 0.65), None, 0.0, 2.0 * PI, 0.0, PI, 16.0, 12.0, 12.0, None)
                    .ok_or_else(|| err("Invalid EllipsoidOutlineGeometry"))?,
            ),
            "createCylinderGeometry" => GeometryDescription::CylinderGeometry(
                CylinderGeometry::new(1.4, 0.35, 0.85, 28, VertexFormat::POSITION_AND_NORMAL, None).unwrap(),
            ),
            "createCylinderOutlineGeometry" => GeometryDescription::CylinderOutlineGeometry(
                CylinderOutlineGeometry::new(1.4, 0.35, 0.85, 24, 12.0, None).unwrap(),
            ),
            "createPlaneGeometry" => GeometryDescription::PlaneGeometry(PlaneGeometry::new(VertexFormat::POSITION_AND_NORMAL)),
            "createPlaneOutlineGeometry" => GeometryDescription::PlaneOutlineGeometry(PlaneOutlineGeometry),
            "createFrustumGeometry" => {
                let f = Frustum::Perspective(PerspectiveFrustum { fov: FRAC_PI_3, aspect_ratio: 1.4, near: 0.4, far: 1.6, x_offset: 0.0, y_offset: 0.0 });
                GeometryDescription::FrustumGeometry(FrustumGeometry::new(f, Cartesian3::new(0.0, 0.0, -0.8), Quaternion::IDENTITY, VertexFormat::POSITION_AND_NORMAL))
            }
            "createFrustumOutlineGeometry" => {
                let f = Frustum::Perspective(PerspectiveFrustum { fov: FRAC_PI_3, aspect_ratio: 1.4, near: 0.4, far: 1.6, x_offset: 0.0, y_offset: 0.0 });
                GeometryDescription::FrustumOutlineGeometry(FrustumOutlineGeometry::new(f, Cartesian3::new(0.0, 0.0, -0.8), Quaternion::IDENTITY))
            }
            "createCircleGeometry" => {
                let mut opts = EllipseGeometryOptions::new(center, 0.55, 0.55);
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.18);
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::CircleGeometry(CircleGeometry::new(0.55, opts).unwrap())
            }
            "createCircleOutlineGeometry" => {
                let mut opts = EllipseOutlineOptions::new(center, 0.55, 0.55);
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.18);
                GeometryDescription::CircleOutlineGeometry(CircleOutlineGeometry::new(0.55, opts).unwrap())
            }
            "createEllipseGeometry" => {
                let mut opts = EllipseGeometryOptions::new(center, 0.65, 0.38);
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.16);
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::EllipseGeometry(EllipseGeometry::new(opts).unwrap())
            }
            "createEllipseOutlineGeometry" => {
                let mut opts = EllipseOutlineOptions::new(center, 0.65, 0.38);
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.16);
                GeometryDescription::EllipseOutlineGeometry(EllipseOutlineGeometry::new(opts).unwrap())
            }
            "createRectangleGeometry" => {
                let mut opts = RectangleGeometryOptions::new(Rectangle::from_degrees(-20.0, -14.0, 20.0, 14.0));
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.16);
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::RectangleGeometry(RectangleGeometry::new(opts))
            }
            "createRectangleOutlineGeometry" => {
                let mut opts = rectangle_outline_geometry::RectangleOutlineOptions::new(Rectangle::from_degrees(-20.0, -14.0, 20.0, 14.0));
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.16);
                GeometryDescription::RectangleOutlineGeometry(RectangleOutlineGeometry::new(opts))
            }
            "createPolygonGeometry" => {
                let mut opts = PolygonGeometryOptions::new(PolygonHierarchy::new(surface_ring(18.0), vec![]));
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.18);
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::PolygonGeometry(PolygonGeometry::new(opts))
            }
            "createPolygonOutlineGeometry" => {
                let mut opts = PolygonOutlineOptions::new(PolygonHierarchy::new(surface_ring(18.0), vec![]));
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.18);
                GeometryDescription::PolygonOutlineGeometry(PolygonOutlineGeometry::new(opts))
            }
            "createCoplanarPolygonGeometry" => {
                let pts = vec![
                    Cartesian3::new(-0.8, -0.6, 0.0),
                    Cartesian3::new(0.8, -0.5, 0.0),
                    Cartesian3::new(0.6, 0.7, 0.0),
                    Cartesian3::new(-0.7, 0.6, 0.0),
                ];
                let mut opts = CoplanarPolygonGeometryOptions::new(PolygonHierarchy::new(pts, vec![]));
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::CoplanarPolygonGeometry(CoplanarPolygonGeometry::new(opts))
            }
            "createCoplanarPolygonOutlineGeometry" => {
                let pts = vec![
                    Cartesian3::new(-0.8, -0.6, 0.0),
                    Cartesian3::new(0.8, -0.5, 0.0),
                    Cartesian3::new(0.6, 0.7, 0.0),
                    Cartesian3::new(-0.7, 0.6, 0.0),
                ];
                GeometryDescription::CoplanarPolygonOutlineGeometry(CoplanarPolygonOutlineGeometry::new(PolygonHierarchy::new(pts, vec![])))
            }
            "createCorridorGeometry" => {
                let mut opts = CorridorGeometryOptions::new(path_pts, 0.22);
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.12);
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::CorridorGeometry(CorridorGeometry::new(opts))
            }
            "createCorridorOutlineGeometry" => {
                let mut opts = CorridorOutlineGeometryOptions::new(path_pts, 0.22);
                opts.ellipsoid = e;
                opts.extruded_height = Some(0.12);
                GeometryDescription::CorridorOutlineGeometry(CorridorOutlineGeometry::new(opts))
            }
            "createWallGeometry" => GeometryDescription::WallGeometry(WallGeometry::from_constant_heights(
                path_pts,
                Some(0.0),
                Some(0.28),
                e,
                VertexFormat::POSITION_AND_NORMAL,
            )),
            "createWallOutlineGeometry" => GeometryDescription::WallOutlineGeometry(
                WallOutlineGeometry::from_constant_heights(path_pts, Some(0.0), Some(0.28), e),
            ),
            "createPolylineVolumeGeometry" => {
                let shape = vec![
                    Cartesian2::new(-0.07, -0.07),
                    Cartesian2::new(0.07, -0.07),
                    Cartesian2::new(0.07, 0.07),
                    Cartesian2::new(-0.07, 0.07),
                ];
                let mut opts = PolylineVolumeGeometryOptions::new(path_pts, shape);
                opts.ellipsoid = e;
                opts.vertex_format = VertexFormat::POSITION_AND_NORMAL;
                GeometryDescription::PolylineVolumeGeometry(PolylineVolumeGeometry::new(opts))
            }
            "createPolylineVolumeOutlineGeometry" => {
                let shape = vec![
                    Cartesian2::new(-0.07, -0.07),
                    Cartesian2::new(0.07, -0.07),
                    Cartesian2::new(0.07, 0.07),
                    Cartesian2::new(-0.07, 0.07),
                ];
                GeometryDescription::PolylineVolumeOutlineGeometry(PolylineVolumeOutlineGeometry::new(
                    path_pts,
                    shape,
                    Some(e),
                    None,
                    None,
                ))
            }
            "createSimplePolylineGeometry" => {
                let mut opts = simple_polyline_geometry::SimplePolylineGeometryOptions::new(path_pts);
                opts.ellipsoid = e;
                GeometryDescription::SimplePolylineGeometry(SimplePolylineGeometry::new(opts))
            }
            "createPolylineGeometry" => {
                let mut opts = PolylineGeometryOptions::new(path_pts);
                opts.ellipsoid = e;
                opts.width = 4.0;
                GeometryDescription::PolylineGeometry(PolylineGeometry::new(opts))
            }
            "createGroundPolylineGeometry" => {
                let g_pts: Vec<Cartesian3> = [(-105.5, 39.5), (-105.0, 40.0), (-104.5, 39.7)]
                    .iter()
                    .map(|&(lon, lat)| Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(lon, lat, 0.0)))
                    .collect();
                GeometryDescription::GroundPolylineGeometry(
                    GroundPolylineGeometry::new(g_pts).ok_or_else(|| err("Invalid GroundPolylineGeometry"))?,
                )
            }
            _ => return Err(err(format!("Unknown worker module: {module_name}"))),
        };

        let mut geom = desc.create_geometry().ok_or_else(|| err("Worker returned None"))?;
        if geom.primitive_type == PrimitiveType::Triangles {
            geometry_pipeline::reorder_for_post_vertex_cache(&mut geom, None);
        }
        let elapsed_ms = js_sys::Date::now() - t0;
        let (mut interleaved, indices, wire_indices) =
            extract_geometry_pos_norm_idx(&geom).ok_or_else(|| err("Could not extract positions"))?;

        // Normalize into [-1, 1] bounding sphere for the 3D inspector viewport
        let n_verts = interleaved.len() / 6;
        let mut min_v = [f32::INFINITY; 3];
        let mut max_v = [f32::NEG_INFINITY; 3];
        for i in 0..n_verts {
            for k in 0..3 {
                min_v[k] = min_v[k].min(interleaved[i * 6 + k]);
                max_v[k] = max_v[k].max(interleaved[i * 6 + k]);
            }
        }
        let center_v = [(min_v[0] + max_v[0]) * 0.5, (min_v[1] + max_v[1]) * 0.5, (min_v[2] + max_v[2]) * 0.5];
        let span = ((max_v[0] - min_v[0]).max(max_v[1] - min_v[1]).max(max_v[2] - min_v[2]) * 0.5).max(1e-6);
        for i in 0..n_verts {
            for k in 0..3 {
                interleaved[i * 6 + k] = (interleaved[i * 6 + k] - center_v[k]) / span;
            }
        }

        let is_lines = geom.primitive_type == PrimitiveType::Lines || geom.primitive_type == PrimitiveType::LineLoop || geom.primitive_type == PrimitiveType::LineStrip;
        Ok(json!({
            "module": module_name,
            "primitiveType": format!("{:?}", geom.primitive_type),
            "isLines": is_lines,
            "vertices": n_verts,
            "indices": indices.len(),
            "elapsedMs": elapsed_ms,
            "boundingSphere": geom.bounding_sphere.map(|b| json!({"center": [b.center.x, b.center.y, b.center.z], "radius": b.radius})),
            "interleaved": interleaved,
            "drawIndices": if is_lines { indices } else { indices },
            "wireIndices": wire_indices,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub fn worker_terrain_build(pattern: &str, grid_size: usize, exaggeration: f64, skirt_height: f64) -> Result<String, JsValue> {
        let t0 = js_sys::Date::now();
        let n = grid_size.clamp(8, 65);
        let mut heights = Vec::with_capacity(n * n);
        for row in 0..n {
            for col in 0..n {
                let u = col as f64 / (n - 1) as f64;
                let v = row as f64 / (n - 1) as f64;
                let x = (u - 0.5) * 2.0;
                let y = (v - 0.5) * 2.0;
                let r = (x * x + y * y).sqrt();
                let h = match pattern {
                    "crater" => {
                        let rim = (-((r - 0.55) * 6.5).powi(2)).exp() * 2200.0;
                        let bowl = (-(r * 2.8).powi(2)).exp() * -1400.0;
                        let peak = (-(r * 10.0).powi(2)).exp() * 750.0;
                        1600.0 + rim + bowl + peak + (x * 9.0).sin() * (y * 9.0).cos() * 140.0
                    }
                    "canyon" => {
                        let trench = (-((x - 0.25 * (y * 3.5).sin()) * 5.0).powi(2)).exp() * -2400.0;
                        let plateau = 2600.0 + (x * 5.0).cos() * 250.0 + (y * 7.0).sin() * 220.0;
                        plateau + trench
                    }
                    "waves" => 1200.0 + (x * 6.0).sin() * 700.0 + (y * 6.0).cos() * 700.0 + ((x + y) * 10.0).sin() * 240.0,
                    _ => {
                        // Alpine peaks
                        let p1 = (-((x + 0.3).powi(2) + (y - 0.2).powi(2)) * 4.5).exp() * 3800.0;
                        let p2 = (-((x - 0.4).powi(2) + (y + 0.3).powi(2)) * 6.0).exp() * 2900.0;
                        let ridge = (x * 5.0 + y * 3.0).sin().abs() * 650.0;
                        400.0 + p1 + p2 + ridge
                    }
                };
                heights.push(h as f32);
            }
        }
        let rect_deg = Rectangle::from_degrees(-106.0, 39.0, -105.0, 40.0);
        let rect_rad = Rectangle::from_degrees(-106.0, 39.0, -105.0, 40.0);
        let typed = TypedArray::Float32(heights.clone());
        let mut opts = HeightmapTessellatorOptions::new(&typed, n, n, rect_deg, skirt_height);
        opts.is_geographic = true;
        opts.exaggeration = exaggeration.clamp(0.2, 10.0);
        let res = heightmap_tessellator::compute_vertices(&opts).ok_or_else(|| err("HeightmapTessellator failed"))?;

        // Build HeightmapTerrainData and test interpolate_height at center & corners
        let td = HeightmapTerrainData::new(HeightmapTerrainDataOptions::new(typed, n, n))
            .ok_or_else(|| err("Invalid HeightmapTerrainData"))?;
        let center_h = td.interpolate_height(&rect_rad, to_radians(-105.5), to_radians(39.5));

        // Build clean normalized local 3D mesh for the interactive WebGL2 terrain preview
        let mut verts = Vec::with_capacity(n * n * 7); // x,y,z, nx,ny,nz, h_norm
        let min_h = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let max_h = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let h_span = (max_h - min_h).max(1.0);
        let y_scale = 0.45 * (exaggeration as f32).clamp(0.2, 6.0);
        let sample_y = |r: isize, c: isize| -> f32 {
            let rr = r.clamp(0, n as isize - 1) as usize;
            let cc = c.clamp(0, n as isize - 1) as usize;
            ((heights[rr * n + cc] - min_h) / h_span) * y_scale
        };
        for row in 0..n {
            for col in 0..n {
                let x = (col as f32 / (n - 1) as f32 - 0.5) * 2.0;
                let z = (row as f32 / (n - 1) as f32 - 0.5) * 2.0;
                let h_norm = (heights[row * n + col] - min_h) / h_span;
                let y = h_norm * y_scale - y_scale * 0.4;
                let dx = sample_y(row as isize, col as isize + 1) - sample_y(row as isize, col as isize - 1);
                let dz = sample_y(row as isize + 1, col as isize) - sample_y(row as isize - 1, col as isize);
                let step = 4.0 / (n - 1) as f32;
                let nx = -dx / step;
                let nz = -dz / step;
                let ny = 1.0f32;
                let l = (nx * nx + ny * ny + nz * nz).sqrt();
                verts.extend_from_slice(&[x, y, z, nx / l, ny / l, nz / l, h_norm]);
            }
        }
        let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
        for row in 0..(n - 1) {
            for col in 0..(n - 1) {
                let i = (row * n + col) as u32;
                indices.extend_from_slice(&[i, i + n as u32, i + 1, i + 1, i + n as u32, i + n as u32 + 1]);
            }
        }
        let elapsed_ms = js_sys::Date::now() - t0;
        Ok(json!({
            "pattern": pattern,
            "grid": n,
            "tessellatorFloatCount": res.vertices.len(),
            "encodingStride": res.encoding.stride,
            "minHeight": res.minimum_height,
            "maxHeight": res.maximum_height,
            "centerInterpolatedHeight": center_h,
            "boundingSphereRadius": res.bounding_sphere_3d.radius,
            "elapsedMs": elapsed_ms,
            "previewVertices": verts,
            "previewIndices": indices,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub fn worker_gaussian_splats(count: usize, yaw_deg: f64, pitch_deg: f64) -> Result<String, JsValue> {
        let n = count.clamp(64, 20_000);
        let mut positions = Vec::with_capacity(n * 3);
        let mut scales = Vec::with_capacity(n * 3);
        let mut rotations = Vec::with_capacity(n * 4);
        let mut colors = Vec::with_capacity(n * 4);
        // Deterministic procedural galaxy/nebula splat cloud
        let mut seed = 0x1234_5678u32;
        let mut next_f = || -> f32 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed as f32) / (u32::MAX as f32)
        };
        for i in 0..n {
            let arm = (i % 3) as f32 * (2.0 * std::f32::consts::PI / 3.0);
            let t = (i as f32 / n as f32).powf(0.65);
            let angle = arm + t * 5.2 + (next_f() - 0.5) * 0.55;
            let radius = 0.08 + t * 1.15;
            let px = radius * angle.cos();
            let py = (next_f() - 0.5) * (0.28 * (1.1 - t * 0.7));
            let pz = radius * angle.sin();
            positions.extend_from_slice(&[px, py, pz]);
            scales.extend_from_slice(&[0.03, 0.02, 0.03]);
            rotations.extend_from_slice(&[0.0, 0.0, 0.0, 1.0]);
            let r = (255.0 * (1.0 - t * 0.65)).clamp(40.0, 255.0) as u8;
            let g = (140.0 + 115.0 * (1.0 - (t - 0.4).abs() * 2.0).max(0.0)) as u8;
            let b = (110.0 + 145.0 * t).clamp(80.0, 255.0) as u8;
            colors.extend_from_slice(&[r, g, b, 220]);
        }

        let (sy, cy) = (to_radians(yaw_deg).sin() as f32, to_radians(yaw_deg).cos() as f32);
        let (sp, cp) = (to_radians(pitch_deg).sin() as f32, to_radians(pitch_deg).cos() as f32);
        let mv = [
            cy, sy * sp, sy * cp, 0.0,
            0.0, cp, -sp, 0.0,
            -sy, cy * sp, cy * cp, 0.0,
            0.0, 0.0, -3.2, 1.0,
        ];

        let t_sort = js_sys::Date::now();
        let sorted = workers::gaussian_splat_sorter(&positions, &mv, n, "Index")
            .map_err(err)?
            .unwrap_or_default();
        let sort_ms = js_sys::Date::now() - t_sort;

        let t_tex = js_sys::Date::now();
        let tex = workers::gaussian_splat_texture_generator(&positions, &scales, &rotations, &colors, n).map_err(err)?;
        let tex_ms = js_sys::Date::now() - t_tex;

        // Return up to 4000 sorted splats for interactive WebGL2 point-sprite display
        let draw_n = n.min(4000);
        let mut draw_pos = Vec::with_capacity(draw_n * 3);
        let mut draw_col = Vec::with_capacity(draw_n * 4);
        for &idx in sorted.iter().take(draw_n) {
            let i = idx as usize;
            draw_pos.extend_from_slice(&positions[i * 3..i * 3 + 3]);
            draw_col.extend_from_slice(&[
                colors[i * 4] as f32 / 255.0,
                colors[i * 4 + 1] as f32 / 255.0,
                colors[i * 4 + 2] as f32 / 255.0,
                colors[i * 4 + 3] as f32 / 255.0,
            ]);
        }

        Ok(json!({
            "count": n,
            "sortMs": sort_ms,
            "textureGenMs": tex_ms,
            "textureWidth": tex.width,
            "textureHeight": tex.height,
            "textureU32Len": tex.data.len(),
            "drawCount": draw_n,
            "positions": draw_pos,
            "colors": draw_col,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub fn worker_codecs_test() -> Result<String, JsValue> {
        let mut checks = Vec::new();
        // 1. AttributeCompression oct-encode/decode normal
        let n = Cartesian3::new(0.3, -0.7, 0.648).normalize();
        let oct = AttributeCompression::oct_encode(n);
        let dec = AttributeCompression::oct_decode(oct.x, oct.y);
        let oct_err = (n - dec).magnitude();
        checks.push(json!({"name": "AttributeCompression::oct_encode/decode", "ok": oct_err < 0.02, "detail": format!("err={oct_err:.5}")}));

        // 2. EncodedCartesian3 high/low precision split
        let big = Cartesian3::new(6_378_137.123456, -4_200_111.987654, 1_900_555.444333);
        let enc = EncodedCartesian3::from_cartesian(big);
        let recon = Cartesian3::new(
            f64::from(enc.high.x as f32) + f64::from(enc.low.x as f32),
            f64::from(enc.high.y as f32) + f64::from(enc.low.y as f32),
            f64::from(enc.high.z as f32) + f64::from(enc.low.z as f32),
        );
        let enc_err = (big - recon).magnitude();
        checks.push(json!({"name": "EncodedCartesian3 (RTC f32+f32)", "ok": enc_err < 1e-3, "detail": format!("err={enc_err:.2e} m")}));

        // 3. workers::combine_geometry
        let b1 = BoxGeometry::from_dimensions(Cartesian3::new(2.0, 2.0, 2.0), VertexFormat::POSITION_AND_NORMAL, None).unwrap().create_geometry().unwrap();
        let b2 = SphereGeometry::new(1.5, 12.0, 8.0, VertexFormat::POSITION_AND_NORMAL).unwrap().create_geometry().unwrap();
        let inst1 = GeometryInstance::new(b1, None);
        let inst2 = GeometryInstance::new(b2, None);
        let combined = geometry_pipeline::combine_instances(&[inst1, inst2]).unwrap_or_default();
        checks.push(json!({"name": "GeometryPipeline::combine_instances", "ok": !combined.is_empty(), "detail": format!("{} merged mesh(es)", combined.len())}));

        // 4. workers::task_processor_worker::deliver_task
        let msg = workers::TaskMessage {
            id: Some(json!(7)),
            parameters: Some(json!({"x": 21})),
            base_url: None,
            can_transfer_array_buffer: false,
        };
        let reply = workers::deliver_task(
            &msg,
            workers::WorkerResponse { result: Ok(Some(json!({"doubled": 42}))), transfers: vec![] },
            None,
        );
        let ok_task = reply.message["id"] == 7 && reply.message["result"]["doubled"] == 42;
        checks.push(json!({"name": "workers::task_processor_worker", "ok": ok_task, "detail": "id=7 -> doubled=42"}));

        // 5. EasingFunction + Spline + Quaternion
        let eased = EasingFunction::CubicInOut.evaluate(0.5);
        checks.push(json!({"name": "EasingFunction::CubicInOut(0.5)", "ok": (eased - 0.5).abs() < 1e-12, "detail": format!("{eased:.6}")}));

        // 6. BoundingSphere + OrientedBoundingBox + IntersectionTests
        let bs = BoundingSphere::new(Cartesian3::ZERO, 100.0);
        let hit = intersection_tests::ray_sphere(&Ray::new(Cartesian3::new(0.0, 0.0, 200.0), Cartesian3::new(0.0, 0.0, -1.0)), &bs);
        checks.push(json!({"name": "intersection_tests::ray_sphere", "ok": hit.is_some(), "detail": format!("{hit:?}")}));

        Ok(json!({"checks": checks}).to_string())
    }

    // =========================================================================
    // 7. WEBGL2 RENDERER & GPU DIAGNOSTICS
    // =========================================================================

    #[wasm_bindgen]
    pub fn renderer_capabilities_report(gl: JsValue) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let limits = ctx.limits().map_err(err)?;
        let caps = ctx.capabilities().map_err(err)?;
        Ok(json!({
            "webgl2": ctx.webgl2(),
            "limits": limits,
            "capabilities": {
                "depthTexture": caps.depth_texture,
                "floatingPointTexture": caps.floating_point_texture,
                "halfFloatingPointTexture": caps.half_floating_point_texture,
                "textureFloatLinear": caps.texture_float_linear,
                "colorBufferFloat": caps.color_buffer_float,
                "s3tc": caps.s3tc,
                "pvrtc": caps.pvrtc,
                "etc": caps.etc,
                "etc1": caps.etc1,
                "astc": caps.astc,
                "bc7": caps.bc7,
                "textureFilterAnisotropic": caps.anisotropic,
            }
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub async fn renderer_run_gpu_suite(gl: JsValue) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let mut results = Vec::new();

        // Test 1: VertexArray::from_geometry (interleaved) + ShaderCache + DrawCommand
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = (|| {
                let mut attributes = GeometryAttributes::default();
                attributes.position = Some(GeometryAttribute::doubles(2, vec![-1., -1., 1., -1., 1., 1., -1., 1.]));
                let mut color = GeometryAttribute::new(ComponentDatatype::UnsignedByte, 4, TypedArray::Uint8(vec![40, 220, 140, 255].repeat(4)));
                color.normalize = true;
                attributes.color = Some(color);
                let geom = Geometry::new(attributes, Some(TypedArray::Uint16(vec![0, 1, 2, 0, 2, 3])), PrimitiveType::Triangles);
                let va = VertexArray::from_geometry(&ctx, &geom, GeometryUploadOptions {
                    interleave: true,
                    attribute_locations: HashMap::from([("position".into(), 0), ("color".into(), 1)]),
                    ..Default::default()
                })?;
                let mut cache = ShaderCache::new(&ctx);
                let prog = cache.get_shader_program(&ShaderProgramOptions {
                    vertex_shader_source: ShaderSource::new("in vec2 position; in vec4 color; out vec4 v_c; void main(){ gl_Position=vec4(position,0.,1.); v_c=color; }"),
                    fragment_shader_source: ShaderSource::new("in vec4 v_c; void main(){ out_FragColor=v_c; }"),
                    attribute_locations: BTreeMap::from([("position".into(), 0), ("color".into(), 1)]),
                })?;
                ClearCommand::all().execute(&ctx, &PassState::default())?;
                DrawCommand::new(&va, &prog).execute(&ctx, &PassState::default())?;
                let px = ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 2, height: 2 })?;
                Ok(format!("RGBA=({},{},{},{})", px[0], px[1], px[2], px[3]))
            })();
            results.push(json!({"name": "VertexArray (interleaved) + ShaderCache + DrawCommand", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        // Test 2: Framebuffer + MultisampleFramebuffer (4x MSAA blit) + PBO Readback
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = (|| {
                let target_tex = Rc::new(Texture::new(&ctx, TextureOptions::rgba(4, 4), None)?);
                let target_fb = Rc::new(Framebuffer::new(&ctx, FramebufferOptions { colors: vec![Attachment::Texture(target_tex.clone())], destroy_attachments: false, ..Default::default() })?);
                let msaa_rb = Rc::new(Renderbuffer::new(&ctx, RenderbufferOptions { format: RenderbufferFormat::Rgba8, width: Some(4), height: Some(4), num_samples: 4 })?);
                let msaa_fb = Rc::new(Framebuffer::new(&ctx, FramebufferOptions { colors: vec![Attachment::Renderbuffer(msaa_rb.clone())], ..Default::default() })?);
                ClearCommand { color: Some([0.2, 0.5, 0.8, 1.0]), framebuffer: Some(msaa_fb.clone()), ..Default::default() }.execute(&ctx, &PassState::default())?;
                let ms = MultisampleFramebuffer::new(&ctx, MultisampleFramebufferOptions {
                    width: 4, height: 4,
                    color_textures: vec![target_tex],
                    color_renderbuffers: vec![msaa_rb],
                    depth_stencil_texture: None, depth_stencil_renderbuffer: None, destroy_attachments: false,
                })?;
                ms.blit_framebuffers(false)?;
                let pbo = ctx.read_pixels_to_pbo(ReadState { region: TextureRegion { x: 0, y: 0, width: 4, height: 4 }, framebuffer: Some(&target_fb) })?;
                let bytes = pbo.get_buffer_data(0, 4)?;
                Ok(format!("4x MSAA blit -> PBO RGBA=({},{},{},{})", bytes[0], bytes[1], bytes[2], bytes[3]))
            })();
            results.push(json!({"name": "MultisampleFramebuffer (4x MSAA) + PBO Readback", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        // Test 3: ComputeEngine + ComputeCommand
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = (|| {
                let out_tex = Rc::new(Texture::new(&ctx, TextureOptions::rgba(4, 4), None)?);
                let mut cmd = ComputeCommand {
                    fragment_shader_source: Some(ShaderSource::new("in vec2 v_textureCoordinates; void main(){ out_FragColor = vec4(1.0, 0.5, 0.25, 1.0); }")),
                    output_texture: Some(out_tex.clone()),
                    ..Default::default()
                };
                let mut engine = ComputeEngine::new(&ctx);
                cmd.execute(&mut engine)?;
                let fb = Framebuffer::new(&ctx, FramebufferOptions { colors: vec![Attachment::Texture(out_tex)], destroy_attachments: false, ..Default::default() })?;
                let px = ctx.read_pixels_from(ReadState { region: TextureRegion { x: 0, y: 0, width: 1, height: 1 }, framebuffer: Some(&fb) })?.bytes;
                Ok(format!("Compute output RGBA=({},{},{},{})", px[0], px[1], px[2], px[3]))
            })();
            results.push(json!({"name": "ComputeEngine + ComputeCommand", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        // Test 4: CubeMap + generateMipmap
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = (|| {
                let faces = CubeFace::ALL.map(|f| TextureSource::Pixels { width: 4, height: 4, bytes: [40 + f.index() as u8 * 30, 120, 220, 255].repeat(16) });
                let mut opts = TextureOptions::rgba(4, 4);
                opts.sampler = Sampler::NEAREST;
                let cube = CubeMap::new(&ctx, opts, Some(faces))?;
                cube.generate_mipmap(MipmapHint::DontCare)?;
                Ok(format!("6 faces uploaded + mipmapped (size {}x{})", cube.width(), cube.height()))
            })();
            results.push(json!({"name": "CubeMap + Mipmap Generation", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        // Test 5: Texture3D volume upload
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = (|| {
                let bytes = [128u8, 64, 255, 255].repeat(64);
                let tex3d = Texture3D::new(&ctx, TextureOptions::rgba(4, 4), 4, Some(&bytes))?;
                Ok(format!("3D Volume {}x{}x{} ({} bytes)", tex3d.width(), tex3d.height(), tex3d.depth(), tex3d.size_in_bytes()))
            })();
            results.push(json!({"name": "Texture3D Volume Allocation", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        // Test 6: TextureAtlas dynamic packing
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = async {
                let mut atlas = TextureAtlas::new(TextureAtlasOptions { initial_size: [16, 16], ..Default::default() })?;
                let a = atlas.add_image("red", TextureSource::Pixels { width: 8, height: 8, bytes: [255, 60, 60, 255].repeat(64) })?;
                let b = atlas.add_image("teal", TextureSource::Pixels { width: 12, height: 8, bytes: [40, 220, 200, 255].repeat(96) })?;
                atlas.update(&ctx)?;
                let ia = a.await?;
                let ib = b.await?;
                let dims = atlas.texture().unwrap().dimensions();
                Ok(format!("Packed ids [{ia}, {ib}] into {}x{} atlas", dims[0], dims[1]))
            }.await;
            results.push(json!({"name": "TextureAtlas Dynamic Packing", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        // Test 7: Shader compile/link error diagnostics
        {
            let t0 = js_sys::Date::now();
            let compile_err = ShaderProgram::new(&ctx, "#version 300 es\nINVALID_GLSL", "#version 300 es\nprecision highp float;out vec4 c;void main(){c=vec4(1.);}", &[]).is_err();
            results.push(json!({"name": "ShaderProgram Driver Diagnostics", "ok": compile_err, "ms": js_sys::Date::now() - t0, "detail": "Invalid GLSL rejected with driver log"}));
        }

        // Test 8: GPU PickId registry & color encoding
        {
            let t0 = js_sys::Date::now();
            let res: Result<String, RuntimeError> = (|| {
                let id = ctx.create_pick_id(Rc::new("CesiumRS-Picked-Entity".to_string()))?;
                let fetched = ctx.pick_object::<String>(id.key).map(|o| (*o).clone());
                Ok(format!("PickId key={} -> {:?}", id.key, fetched.unwrap_or_default()))
            })();
            results.push(json!({"name": "Context PickId & Object Lookup", "ok": res.is_ok(), "ms": js_sys::Date::now() - t0, "detail": res.unwrap_or_else(|e| e.to_string())}));
        }

        Ok(json!({"results": results}).to_string())
    }

    #[wasm_bindgen]
    pub fn renderer_compute_demo(gl: JsValue, width: u32, height: u32, time_sec: f32) -> Result<(), JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let out_tex = Rc::new(Texture::new(&ctx, TextureOptions::rgba(width, height), None).map_err(err)?);
        let frag = ShaderSource::new(
            "in vec2 v_textureCoordinates;
             uniform float u_time;
             void main() {
                 vec2 uv = v_textureCoordinates * 2.0 - 1.0;
                 float r = length(uv);
                 float a = atan(uv.y, uv.x);
                 float w = sin(r * 14.0 - u_time * 3.0) * 0.5 + 0.5;
                 float s = cos(a * 6.0 + u_time * 1.6) * 0.5 + 0.5;
                 vec3 col = mix(vec3(0.06, 0.14, 0.28), vec3(0.95, 0.68, 0.18), w * s);
                 col += vec3(0.15, 0.72, 0.92) * smoothstep(0.65, 0.0, r) * (0.4 + 0.6 * w);
                 out_FragColor = vec4(col, 1.0);
             }",
        );
        let mut cmd = ComputeCommand {
            fragment_shader_source: Some(frag),
            output_texture: Some(out_tex.clone()),
            uniform_map: Some(Rc::new(move || Ok(vec![("u_time".into(), UniformValue::Float(vec![time_sec]))]))),
            ..Default::default()
        };
        let mut engine = ComputeEngine::new(&ctx);
        cmd.execute(&mut engine).map_err(err)?;

        let vp = Viewport { x: 0, y: 0, width, height };
        let quad = ViewportQuad::new(&ctx).map_err(err)?;
        let prog = ShaderProgram::from_sources(
            &ctx,
            &ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(
                "in vec2 v_textureCoordinates; uniform sampler2D u_tex; void main(){ out_FragColor = texture(u_tex, v_textureCoordinates); }",
            )),
        )
        .map_err(err)?;
        let mut draw = DrawCommand::new(quad.vertex_array(), &prog);
        draw.textures = vec![("u_tex".into(), &*out_tex)];
        draw.execute(&ctx, &PassState { viewport: Some(vp), ..Default::default() }).map_err(err)?;
        Ok(())
    }

    #[wasm_bindgen]
    pub async fn renderer_atlas_demo(gl: JsValue, size: u32) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let mut atlas = TextureAtlas::new(TextureAtlasOptions { initial_size: [64, 64], border_width_in_pixels: 2.0, ..Default::default() }).map_err(err)?;
        let swatches: [(&str, u32, u32, [u8; 4]); 8] = [
            ("amber", 48, 36, [235, 168, 42, 255]),
            ("cyan", 36, 48, [64, 215, 240, 255]),
            ("rose", 52, 28, [242, 85, 128, 255]),
            ("emerald", 40, 40, [52, 211, 138, 255]),
            ("violet", 60, 24, [152, 110, 248, 255]),
            ("sky", 32, 54, [66, 155, 248, 255]),
            ("orange", 44, 32, [248, 125, 48, 255]),
            ("lime", 36, 28, [164, 232, 62, 255]),
        ];
        let mut futures = Vec::new();
        for (name, w, h, rgba) in swatches {
            let mut bytes = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let border = x < 2 || y < 2 || x + 2 >= w || y + 2 >= h;
                    if border {
                        bytes.extend_from_slice(&[245, 250, 255, 255]);
                    } else {
                        let grad = 0.75 + 0.25 * (x as f32 / w as f32);
                        bytes.extend_from_slice(&[
                            (rgba[0] as f32 * grad) as u8,
                            (rgba[1] as f32 * grad) as u8,
                            (rgba[2] as f32 * grad) as u8,
                            255,
                        ]);
                    }
                }
            }
            futures.push(atlas.add_image(name, TextureSource::Pixels { width: w, height: h, bytes }).map_err(err)?);
        }
        atlas.update(&ctx).map_err(err)?;
        for f in futures {
            f.await.map_err(err)?;
        }
        let tex = atlas.texture().ok_or_else(|| err("Atlas missing texture"))?.clone();
        let vp = Viewport { x: 0, y: 0, width: size, height: size };
        ClearCommand { color: Some([0.05, 0.07, 0.11, 1.0]), ..Default::default() }
            .execute(&ctx, &PassState { viewport: Some(vp), ..Default::default() })
            .map_err(err)?;
        let quad = ViewportQuad::new(&ctx).map_err(err)?;
        let prog = ShaderProgram::from_sources(
            &ctx,
            &ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(
                "in vec2 v_textureCoordinates; uniform sampler2D u_tex; void main(){ out_FragColor = texture(u_tex, v_textureCoordinates); }",
            )),
        )
        .map_err(err)?;
        let mut draw = DrawCommand::new(quad.vertex_array(), &prog);
        draw.textures = vec![("u_tex".into(), &*tex)];
        draw.execute(&ctx, &PassState { viewport: Some(vp), ..Default::default() }).map_err(err)?;
        Ok(json!({"images": atlas.number_of_images(), "atlasSize": tex.dimensions(), "bytes": atlas.size_in_bytes()}).to_string())
    }

    // =========================================================================
    // 8. 307 GLSL SHADERS REGISTRY EXPLORER
    // =========================================================================

    #[wasm_bindgen]
    pub fn shaders_list() -> String {
        let items: Vec<Value> = shaders::registry()
            .iter()
            .map(|m| {
                let category = m.id.split('/').nth(1).unwrap_or("Root");
                json!({
                    "id": m.id,
                    "name": m.name,
                    "category": category,
                    "lines": m.source.lines().count(),
                    "bytes": m.source.len(),
                })
            })
            .collect();
        json!({ "total": items.len(), "shaders": items }).to_string()
    }

    #[wasm_bindgen]
    pub fn shaders_get(id: &str) -> Result<String, JsValue> {
        let m = shaders::shader(id).ok_or_else(|| err(format!("Shader not found: {id}")))?;
        let webgl1 = demodernize_shader(m.source, true);
        Ok(json!({
            "id": m.id,
            "name": m.name,
            "source": m.source,
            "demodernizedWebgl1": webgl1,
        })
        .to_string())
    }

    #[wasm_bindgen]
    pub fn shaders_test_compile(gl: JsValue, source: &str, is_fragment: bool) -> Result<String, JsValue> {
        let ctx = Context::new_webgl2(BrowserGl::new(gl).map_err(err)?);
        let opts = if is_fragment {
            ViewportQuad::<BrowserGl>::shader_options(ShaderSource::new(source))
        } else {
            ShaderProgramOptions {
                vertex_shader_source: ShaderSource::new(source),
                fragment_shader_source: ShaderSource::new("void main(){ out_FragColor = vec4(1.0); }"),
                attribute_locations: BTreeMap::new(),
            }
        };
        match ShaderProgram::from_sources(&ctx, &opts) {
            Ok(prog) => {
                let attrs = prog.vertex_attributes().len();
                let uniforms = prog.uniforms().len();
                let _ = prog.destroy();
                Ok(json!({"ok": true, "attributes": attrs, "uniforms": uniforms}).to_string())
            }
            Err(e) => Ok(json!({"ok": false, "error": e.message}).to_string()),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Build for wasm32 with: bash scripts/build-test-site.sh");
}
