//! CesiumJS Renderbuffer, including multisample storage.
use super::{context::*, RenderbufferFormat};
use crate::core::webgl_constants as gl;
use std::rc::Rc;
#[derive(Clone, Copy, Debug)]
pub struct RenderbufferOptions {
    pub format: RenderbufferFormat,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub num_samples: u32,
}
impl Default for RenderbufferOptions {
    fn default() -> Self {
        Self { format: RenderbufferFormat::Rgba4, width: None, height: None, num_samples: 1 }
    }
}
pub struct Renderbuffer<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    format: RenderbufferFormat,
    width: u32,
    height: u32,
    samples: u32,
}
impl<G: Gl> Renderbuffer<G> {
    pub fn new(context: &Context<G>, options: RenderbufferOptions) -> Result<Self> {
        let drawing = context.host().drawing_buffer_size()?;
        let width = options.width.unwrap_or(drawing[0]);
        let height = options.height.unwrap_or(drawing[1]);
        ensure(width > 0 && height > 0, "Renderbuffer dimensions must be positive")?;
        size::<G::Object>(width as usize)?;
        size::<G::Object>(height as usize)?;
        if let GlValue::Number(max) = context.call("getParameter", &[gl::MAX_RENDERBUFFER_SIZE.into()])? {
            ensure(width as f64 <= max && height as f64 <= max, "Renderbuffer exceeds MAX_RENDERBUFFER_SIZE")?;
        }
        let resource = Rc::new(Resource::create(context, "createRenderbuffer", &[], "deleteRenderbuffer")?);
        context.call("bindRenderbuffer", &[gl::RENDERBUFFER.into(), resource.value()?])?;
        let mut args = vec![gl::RENDERBUFFER.into()];
        if options.num_samples > 1 {
            args.push(options.num_samples.into());
        }
        args.extend([(options.format as u32).into(), width.into(), height.into()]);
        let result = context.call(
            if options.num_samples > 1 { "renderbufferStorageMultisample" } else { "renderbufferStorage" },
            &args,
        );
        context.call("bindRenderbuffer", &[gl::RENDERBUFFER.into(), GlValue::Null])?;
        result?;
        Ok(Self { resource, format: options.format, width, height, samples: options.num_samples })
    }
    pub fn format(&self) -> RenderbufferFormat {
        self.format
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn num_samples(&self) -> u32 {
        self.samples
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
}
