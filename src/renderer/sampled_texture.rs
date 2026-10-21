//! Common sampling boundary for 2D, cube and volume resources.
use super::{context::*, *};
use crate::core::webgl_constants as gl;
use std::rc::Rc;
pub trait SampledTexture<G: Gl> {
    fn context(&self) -> &Context<G>;
    fn target(&self) -> u32;
    fn bind(&self, unit: u32) -> Result<()>;
    fn is_destroyed(&self) -> bool;
}
macro_rules! sampled {
    ($ty:ident,$target:ident) => {
        impl<G: Gl> SampledTexture<G> for $ty<G> {
            fn context(&self) -> &Context<G> {
                &self.resource.context
            }
            fn target(&self) -> u32 {
                gl::$target
            }
            fn bind(&self, unit: u32) -> Result<()> {
                $ty::bind(self, unit)
            }
            fn is_destroyed(&self) -> bool {
                $ty::is_destroyed(self)
            }
        }
    };
}
sampled!(Texture, TEXTURE_2D);
sampled!(CubeMap, TEXTURE_CUBE_MAP);
sampled!(Texture3D, TEXTURE_3D);
pub enum TextureBinding<G: Gl> {
    Texture(Rc<Texture<G>>),
    CubeMap(Rc<CubeMap<G>>),
    Volume(Rc<Texture3D<G>>),
}
impl<G: Gl> Clone for TextureBinding<G> {
    fn clone(&self) -> Self {
        match self {
            Self::Texture(t) => Self::Texture(t.clone()),
            Self::CubeMap(t) => Self::CubeMap(t.clone()),
            Self::Volume(t) => Self::Volume(t.clone()),
        }
    }
}
impl<G: Gl> TextureBinding<G> {
    pub fn sampled(&self) -> &dyn SampledTexture<G> {
        match self {
            Self::Texture(t) => t.as_ref(),
            Self::CubeMap(t) => t.as_ref(),
            Self::Volume(t) => t.as_ref(),
        }
    }
}
