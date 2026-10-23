//! CesiumJS `PickDepth`: merge scene and globe depth into a readable RGBA target.
use super::framebuffer_helpers::*;
use crate::{core::RuntimeError, renderer::*};
use std::rc::Rc;
const COPY_DEPTH: &str = r#"uniform highp sampler2D colorTexture;
uniform highp sampler2D globeDepthTexture;
in vec2 v_textureCoordinates;
void main()
{
    vec4 globeDepthPacked = texture(globeDepthTexture, v_textureCoordinates);
    float globeDepth = czm_unpackDepth(globeDepthPacked);
    float depth = texture(colorTexture, v_textureCoordinates).r;
    out_FragColor = czm_branchFreeTernary(globeDepth <= 0.0 || globeDepth >= 1.0 || depth < globeDepth && depth > 0.0 && depth < 1.0,
        czm_packDepth(depth), globeDepthPacked);
}
"#;

pub struct PickDepth<G: Gl> {
    framebuffer: FramebufferManager<G>,
    texture: Option<Rc<Texture<G>>>,
    command: Option<ViewportCommand<G>>,
    zero: Option<Rc<Texture<G>>>,
    destroyed: bool,
}
impl<G: Gl> PickDepth<G> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            framebuffer: FramebufferManager::new(FramebufferManagerOptions::default())?,
            texture: None,
            command: None,
            zero: None,
            destroyed: false,
        })
    }
    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.framebuffer.framebuffer()
    }
    pub fn texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.framebuffer.color_texture(0)
    }
    pub fn update(&mut self, context: &Context<G>, depth_texture: Rc<Texture<G>>) -> Result<()> {
        alive(self.destroyed)?;
        self.framebuffer.update(context, depth_texture.width(), depth_texture.height(), None, None, None)?;
        self.texture = Some(depth_texture);
        Ok(())
    }
    pub fn execute_copy_depth(
        &mut self,
        context: &Context<G>,
        pass: &PassState<G>,
        globe_depth: Option<Rc<Texture<G>>>,
    ) -> Result<()> {
        alive(self.destroyed)?;
        if self.command.is_none() {
            self.command = Some(
                context
                    .create_viewport_quad_command(ShaderSource::new(COPY_DEPTH), ViewportCommandOptions::default())?,
            );
        }
        let globe = if let Some(t) = globe_depth {
            t
        } else {
            if self.zero.is_none() {
                let mut o = TextureOptions::rgba(1, 1);
                o.flip_y = false;
                o.sampler = Sampler::NEAREST;
                self.zero = Some(Rc::new(Texture::new(context, o, Some(&[0; 4]))?));
            }
            self.zero.as_ref().expect("initialized").clone()
        };
        let texture =
            self.texture.clone().ok_or_else(|| RuntimeError::new("PickDepth.update must be called before copying"))?;
        let command = self.command.as_mut().expect("initialized");
        command.options.framebuffer = self.framebuffer.framebuffer().cloned();
        command.textures = vec![
            ("colorTexture".into(), TextureBinding::Texture(texture)),
            ("globeDepthTexture".into(), TextureBinding::Texture(globe)),
        ];
        command.execute(context, pass)
    }
    pub fn decode_depth(pixels: [u8; 4]) -> f64 {
        pixels.into_iter().zip([1., 1. / 255., 1. / 65025., 1. / 16581375.]).map(|(p, s)| p as f64 / 255. * s).sum()
    }
    pub fn get_depth(&self, context: &Context<G>, x: f64, y: f64) -> Result<Option<f64>> {
        alive(self.destroyed)?;
        let Some(framebuffer) = self.framebuffer() else { return Ok(None) };
        let x = x.max(0.) as u32;
        let y = y.max(0.) as u32;
        let [w, h] = framebuffer.dimensions().unwrap_or([0, 0]);
        if x >= w || y >= h {
            return Ok(Some(0.));
        }
        let read = context.read_pixels_from(ReadState {
            framebuffer: Some(framebuffer),
            region: TextureRegion { x, y, width: 1, height: 1 },
        })?;
        Ok(Some(Self::decode_depth(read.bytes[..4].try_into().expect("RGBA read"))))
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.framebuffer.destroy()?;
            destroy_command(&mut self.command)?;
            if let Some(zero) = self.zero.take() {
                zero.destroy()?;
            }
            self.texture = None;
            self.destroyed = true;
        }
        Ok(())
    }
}
