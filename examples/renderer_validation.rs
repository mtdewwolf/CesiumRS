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
        let pass: PassState = serde_json::from_str(pass).map_err(error)?;
        state.apply(&ctx, &pass, ctx.host().drawing_buffer_size().map_err(error)?).map_err(error)
    }
    #[wasm_bindgen]
    pub fn renderer_draw(gl: JsValue, config: &str) -> Result<String, JsValue> {
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
        let fs = format!("#version 300 es\nprecision highp float;\nin vec4 v_color; in vec2 v_uv; uniform vec4 tint; uniform int enabled; {}out vec4 result; void main() {{result= (enabled==1 ? tint : vec4(0.)) * v_color {};}}", if textured {"uniform sampler2D image; "} else {""},if textured {"* texture(image,v_uv)"} else {""});
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
            let t = Texture::new(&ctx, opts, Some(&pixels)).map_err(error)?;
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
        let pass: PassState = serde_json::from_value(c.get("pass").cloned().unwrap_or(json!({}))).map_err(error)?;
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
        let pass: PassState = serde_json::from_str(pass).map_err(error)?;
        ClearCommand { color: Some([0.1, 0.2, 0.3, 1.]), ..Default::default() }
            .execute(&ctx, &PassState::default())
            .map_err(error)?;
        ClearCommand { color: Some([1., 0.5, 0., 0.25]), depth: Some(0.5), stencil: Some(3), render_state: state }
            .execute(&ctx, &pass)
            .map_err(error)?;
        Ok(serde_json::to_string(&ctx.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?)
            .unwrap())
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
