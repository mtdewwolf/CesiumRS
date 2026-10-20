//! CesiumJS Renderer constants. GL values are shared with Core.
use crate::core::webgl_constants as gl;
macro_rules! gl_enum {
    ($name:ident { $($variant:ident = $constant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u32)]
        pub enum $name { $($variant = gl::$constant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn from_gl(value: u32) -> Option<Self> { Self::ALL.iter().copied().find(|v| *v as u32 == value) }
            pub fn validate(value: u32) -> bool { Self::from_gl(value).is_some() }
            pub fn name(self) -> &'static str { match self { $(Self::$variant => stringify!($constant)),+ } }
        }
    };
}
gl_enum!(BufferUsage { StreamDraw = STREAM_DRAW, StaticDraw = STATIC_DRAW, DynamicDraw = DYNAMIC_DRAW, DynamicRead = DYNAMIC_READ });
gl_enum!(TextureWrap { ClampToEdge = CLAMP_TO_EDGE, Repeat = REPEAT, MirroredRepeat = MIRRORED_REPEAT });
gl_enum!(TextureMinificationFilter { Nearest = NEAREST, Linear = LINEAR, NearestMipmapNearest = NEAREST_MIPMAP_NEAREST, LinearMipmapNearest = LINEAR_MIPMAP_NEAREST, NearestMipmapLinear = NEAREST_MIPMAP_LINEAR, LinearMipmapLinear = LINEAR_MIPMAP_LINEAR });
gl_enum!(TextureMagnificationFilter { Nearest = NEAREST, Linear = LINEAR });
gl_enum!(MipmapHint { DontCare = DONT_CARE, Fastest = FASTEST, Nicest = NICEST });
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Pass {
    Environment = 0,
    Compute = 1,
    Globe = 2,
    TerrainClassification = 3,
    Cesium3DTileEdges = 4,
    Cesium3DTile = 5,
    Cesium3DTileClassification = 6,
    Cesium3DTileClassificationIgnoreShow = 7,
    Opaque = 8,
    Translucent = 9,
    Voxels = 10,
    GaussianSplats = 11,
    Overlay = 12,
}
impl Pass {
    pub const NUMBER_OF_PASSES: u32 = 13;
}
