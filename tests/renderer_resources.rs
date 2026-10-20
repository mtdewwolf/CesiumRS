//! Host-boundary and ownership regressions. Actual GPU behavior is tested by
//! scripts/check-renderer.sh; this host injects deterministic allocation errors.
use cesium_rs::{
    core::{ComponentDatatype, IndexDatatype, RuntimeError},
    renderer::*,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Default)]
struct Calls {
    next: Cell<u32>,
    calls: RefCell<Vec<(String, Vec<GlValue<u32>>)>>,
    fail: RefCell<Option<String>>,
}
#[derive(Clone, Default)]
struct Host(Rc<Calls>);
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.0.calls.borrow_mut().push((name.into(), args.to_vec()));
        if self.0.fail.borrow().as_deref() == Some(name) {
            return Err(RuntimeError::new("injected host failure"));
        }
        if name.starts_with("create") {
            let id = self.0.next.get() + 1;
            self.0.next.set(id);
            return Ok(GlValue::Object(id));
        }
        Ok(match name {
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            "getParameter" => GlValue::Number(16384.),
            _ => GlValue::Null,
        })
    }
}
impl Host {
    fn count(&self, name: &str) -> usize {
        self.0.calls.borrow().iter().filter(|(n, _)| n == name).count()
    }
    fn fail(&self, name: Option<&str>) {
        *self.0.fail.borrow_mut() = name.map(str::to_owned);
    }
}
fn buffer(ctx: &Context<Host>) -> Buffer<Host> {
    Buffer::new(ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(24)).unwrap()
}
#[test]
fn shared_lifetime_and_explicit_invalidation() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let a = buffer(&ctx);
    let b = a.clone();
    drop(a);
    assert_eq!(host.count("deleteBuffer"), 0);
    drop(b);
    assert_eq!(host.count("deleteBuffer"), 1);
    let a = buffer(&ctx);
    let b = a.clone();
    a.destroy().unwrap();
    a.destroy().unwrap();
    assert!(a.is_destroyed() && b.is_destroyed());
    let before = host.0.calls.borrow().len();
    assert!(b.copy_from_array_view(&[1], 0).is_err());
    assert_eq!(host.0.calls.borrow().len(), before);
    drop(a);
    drop(b);
    assert_eq!(host.count("deleteBuffer"), 2);
}
#[test]
fn vao_retains_shared_buffers_and_rejects_wrong_contexts() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let b = buffer(&ctx);
    let va = VertexArray::new(&ctx, vec![VertexAttribute::buffer(0, b.clone(), 3)], None).unwrap();
    drop(b);
    assert_eq!(host.count("deleteBuffer"), 0);
    drop(va);
    assert_eq!(host.count("deleteVertexArray"), 1);
    assert_eq!(host.count("deleteBuffer"), 1);
    let b = buffer(&ctx);
    let foreign = Context::new_webgl2(host.clone()); // same host, distinct context identity
    assert!(VertexArray::new(&foreign, vec![VertexAttribute::buffer(0, b.clone(), 3)], None).is_err());
    let foreign_buffer = buffer(&foreign);
    assert!(b.copy_from_buffer(&foreign_buffer, 0, 0, 4).is_err());
}
#[test]
fn invalid_ranges_and_layouts_are_rejected_before_gl_mutations() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    assert!(Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(0)).is_err());
    assert!(Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(usize::MAX)).is_err());
    assert!(Buffer::new(
        &ctx,
        BufferKind::Index(IndexDatatype::UnsignedInt),
        BufferUsage::StaticDraw,
        BufferData::Size(3)
    )
    .is_err());
    assert_eq!(host.count("createBuffer"), 0);
    let b = buffer(&ctx);
    let before = host.0.calls.borrow().len();
    assert!(b.copy_from_array_view(&[1], usize::MAX).is_err());
    assert!(b.copy_from_array_view(&[1; 25], 0).is_err());
    assert!(b.copy_from_buffer(&b, 0, 1, 4).is_err());
    assert!(b.get_buffer_data(23, 2).is_err());
    assert_eq!(before, host.0.calls.borrow().len());
    let mut a = VertexAttribute::buffer(0, b.clone(), 3);
    a.datatype = ComponentDatatype::Double;
    assert!(VertexArray::new(&ctx, vec![a], None).is_err());
    let mut a = VertexAttribute::buffer(0, b.clone(), 3);
    a.offset = 1;
    assert!(VertexArray::new(&ctx, vec![a], None).is_err());
    assert!(VertexArray::new(&ctx, vec![VertexAttribute::constant(0, vec![1.; 5])], None).is_err());
    assert!(VertexArray::new(
        &ctx,
        vec![VertexAttribute::constant(0, vec![1.]), VertexAttribute::constant(0, vec![2.])],
        None
    )
    .is_err());
    assert_eq!(host.count("createVertexArray"), 0);
}
#[test]
fn allocation_and_setup_failures_release_objects_and_unbind() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    host.fail(Some("bufferData"));
    assert!(Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(12)).is_err());
    assert_eq!(host.count("deleteBuffer"), 1);
    host.fail(None);
    let b = buffer(&ctx);
    host.fail(Some("vertexAttribPointer"));
    assert!(VertexArray::new(&ctx, vec![VertexAttribute::buffer(0, b.clone(), 3)], None).is_err());
    assert_eq!(host.count("deleteVertexArray"), 1);
    host.fail(None);
    host.fail(Some("linkProgram"));
    assert!(ShaderProgram::new(&ctx, "vertex", "fragment", &[]).is_err());
    assert_eq!(host.count("deleteProgram"), 1);
    assert_eq!(host.count("deleteShader"), 2);
    host.fail(None);
    let unbound = host
        .0
        .calls
        .borrow()
        .iter()
        .any(|(name, args)| name == "bindVertexArray" && matches!(args.first(), Some(GlValue::Null)));
    assert!(unbound);
}
#[test]
fn failed_explicit_delete_can_be_retried() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let b = buffer(&ctx);
    host.fail(Some("deleteBuffer"));
    assert!(b.destroy().is_err());
    assert!(!b.is_destroyed());
    host.fail(None);
    b.destroy().unwrap();
    assert!(b.is_destroyed());
}
#[test]
fn draw_dispatch_keeps_cesium_count_and_offset_semantics() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let b = buffer(&ctx);
    let i =
        Buffer::new(&ctx, BufferKind::Index(IndexDatatype::UnsignedInt), BufferUsage::StaticDraw, BufferData::Size(24))
            .unwrap();
    let va = VertexArray::new(&ctx, vec![VertexAttribute::buffer(0, b, 3)], Some(i)).unwrap();
    let p = ShaderProgram::new(&ctx, "vs", "fs", &[]).unwrap();
    let mut draw = DrawCommand::new(&va, &p);
    draw.offset = 2;
    draw.count = Some(100);
    draw.instance_count = 3;
    draw.execute(&ctx, &PassState::default()).unwrap();
    let calls = host.0.calls.borrow();
    let (_, args) = calls.iter().find(|(n, _)| n == "drawElementsInstanced").unwrap();
    for (arg, n) in args.iter().zip([4., 6., 5125., 8., 3.]) {
        assert!(matches!(arg,GlValue::Number(v) if *v==n));
    }
}

#[test]
fn render_states_intern_and_reuse_sparse_transitions() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let va = VertexArray::new(&ctx, vec![VertexAttribute::buffer(0, buffer(&ctx), 3)], None).unwrap();
    let p = ShaderProgram::new(&ctx, "vs", "fs", &[]).unwrap();
    let mut command = DrawCommand::new(&va, &p);
    command.execute(&ctx, &PassState::default()).unwrap();
    let first = host.count("frontFace");
    command.execute(&ctx, &PassState::default()).unwrap();
    assert_eq!(host.count("frontFace"), first);
    command.render_state.depth_mask = false;
    command.execute(&ctx, &PassState::default()).unwrap();
    assert_eq!(host.count("frontFace"), first);
    assert_eq!(host.count("depthMask"), 2);
    command.render_state.depth_mask = true;
    command.execute(&ctx, &PassState::default()).unwrap();
    assert_eq!(host.count("depthMask"), 3);
    ctx.invalidate_render_state();
    command.execute(&ctx, &PassState::default()).unwrap();
    assert_eq!(host.count("frontFace"), first + 1);
    let evaluations = Rc::new(Cell::new(0));
    let count = evaluations.clone();
    command.uniform_map = Some(Rc::new(move || {
        count.set(count.get() + 1);
        Ok(vec![])
    }));
    command.execute(&ctx, &PassState::default()).unwrap();
    command.execute(&ctx, &PassState::default()).unwrap();
    assert_eq!(evaluations.get(), 2);
    command.set_dirty(false);
    assert!(!command.dirty());
    command.metadata.receive_shadows = true;
    assert!(command.dirty());
    let cloned = command.shallow_clone();
    assert!(cloned.dirty());
    assert!(cloned.metadata.receive_shadows);
    assert_eq!(cloned.last_dirty_time, 0.);
}
#[test]
fn shader_and_texture_cache_lifetimes_and_derived_programs() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let mut cache = ShaderCache::new(&ctx);
    let options = ShaderProgramOptions {
        vertex_shader_source: ShaderSource::new("void main(){gl_Position=vec4(0.);}"),
        fragment_shader_source: ShaderSource::new("void main(){out_FragColor=vec4(1.);}"),
        attribute_locations: Default::default(),
    };
    let a = cache.get_shader_program(&options).unwrap();
    let b = cache.get_shader_program(&options).unwrap();
    assert!(Rc::ptr_eq(&a, &b));
    assert_eq!(cache.number_of_shaders(), 1);
    assert_eq!(host.count("createProgram"), 1);
    let derived = cache.create_derived_shader_program(&a, "pick", &options).unwrap();
    let replacement = cache.replace_derived_shader_program(&a, "pick", &options).unwrap();
    assert!(derived.is_destroyed());
    assert!(!replacement.is_destroyed());
    drop(a);
    drop(b);
    cache.destroy_released_shader_programs().unwrap();
    assert!(replacement.is_destroyed());
    assert_eq!(cache.number_of_shaders(), 0);
    assert_eq!(host.count("deleteProgram"), 3);
    let mut textures = TextureCache::default();
    let t = Rc::new(Texture::new(&ctx, TextureOptions::rgba(2, 2), None).unwrap());
    textures.add_texture("test", t.clone()).unwrap();
    drop(t);
    let lease = textures.get_texture("test").unwrap();
    textures.destroy_released_textures().unwrap();
    assert!(!lease.is_destroyed());
    drop(lease);
    textures.destroy_released_textures().unwrap();
    assert_eq!(textures.number_of_textures(), 0);
    assert_eq!(host.count("deleteTexture"), 1);
    let root = cache.get_shader_program(&options).unwrap();
    let child = cache.create_derived_shader_program(&root, "replacement", &options).unwrap();
    root.destroy().unwrap();
    let replacement = cache.get_shader_program(&options).unwrap();
    assert!(!Rc::ptr_eq(&root, &replacement));
    assert!(child.is_destroyed());
    assert!(!replacement.is_destroyed());
    cache.destroy().unwrap();
}
#[test]
fn framebuffer_attachment_ownership_and_cross_context_rejection() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let t = Rc::new(Texture::new(&ctx, TextureOptions::rgba(2, 2), None).unwrap());
    let f = Framebuffer::new(
        &ctx,
        FramebufferOptions {
            colors: vec![Attachment::Texture(t.clone())],
            destroy_attachments: false,
            ..Default::default()
        },
    )
    .unwrap();
    f.destroy().unwrap();
    assert!(!t.is_destroyed());
    let foreign = Context::new_webgl2(host.clone());
    assert!(Framebuffer::new(
        &foreign,
        FramebufferOptions { colors: vec![Attachment::Texture(t.clone())], ..Default::default() }
    )
    .is_err());
    let owned = Framebuffer::new(
        &ctx,
        FramebufferOptions { colors: vec![Attachment::Texture(t.clone())], ..Default::default() },
    )
    .unwrap();
    owned.destroy().unwrap();
    assert!(t.is_destroyed());
    assert_eq!(host.count("deleteTexture"), 1);
    let rb = Rc::new(
        Renderbuffer::new(
            &ctx,
            RenderbufferOptions { format: RenderbufferFormat::Rgba8, width: Some(2), height: Some(2), num_samples: 4 },
        )
        .unwrap(),
    );
    let texture = Rc::new(Texture::new(&ctx, TextureOptions::rgba(2, 2), None).unwrap());
    host.fail(Some("framebufferTexture2D"));
    assert!(MultisampleFramebuffer::new(
        &ctx,
        MultisampleFramebufferOptions {
            width: 2,
            height: 2,
            color_renderbuffers: vec![rb.clone()],
            color_textures: vec![texture.clone()],
            depth_stencil_texture: None,
            depth_stencil_renderbuffer: None,
            destroy_attachments: true
        }
    )
    .is_err());
    host.fail(None);
    assert!(!rb.is_destroyed());
    assert!(!texture.is_destroyed());
}
