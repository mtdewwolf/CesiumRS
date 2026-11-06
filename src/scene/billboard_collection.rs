//! CesiumJS `BillboardCollection`: billboards drawn from one vertex array
//! facade and one texture atlas.
//!
//! The update follows Cesium step for step: compact removed billboards, mark
//! billboards whose image changed, schedule the atlas update after the frame,
//! recompute actual positions for the scene mode, choose a buffer usage per
//! attribute, rewrite every billboard or only the dirty ones (instanced, one
//! vertex per billboard, on contexts with instancing; four vertices otherwise),
//! grow the bounding volume by the largest billboard's size and offsets,
//! compile the shaders the billboards need and queue the draw commands. The
//! vertex bytes, uploads, commands, shader defines, atlas and image states are
//! checked against a capture of the pinned CesiumJS engine on a real WebGL2
//! context in `tests/scene_billboard_collection.rs`.
//!
//! Not ported: clamping to terrain (height references need a scene), the
//! vector-tile batch table and the label-only SDF path's glyph source; the
//! collection writes SDF attributes when [`BillboardCollection::set_sdf`] is on.
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{
    billboard::*,
    billboard_texture::{poll_pending, TextureHost},
    blending_state, is_height_reference_clamp,
    scene::{Result, SceneDrawOptions, ScenePrimitive},
    scene_transforms::SceneTransformState,
    sdf_settings, BlendOption, Cesium3DTilePass, FrameState, HorizontalOrigin, SceneCommand, SceneMode, VerticalOrigin,
};
use crate::{
    core::{
        math, webgl_constants as gl, AttributeCompression, BoundingRectangle, BoundingSphere, Cartesian2, Cartesian3,
        Color, ComponentDatatype, EncodedCartesian3, IndexDatatype, Matrix4, PrimitiveType, RuntimeError,
    },
    renderer::{
        render_state::DepthTest, Buffer, BufferData, BufferKind, BufferUsage, Context, DrawMetadata, FacadeAttribute,
        Gl, Pass, RenderState, SampledTexture, ShaderProgram, ShaderProgramOptions, ShaderSource, TextureAtlas,
        TextureAtlasOptions, UniformFrustum, UniformValue, VertexArrayFacade,
    },
    shaders::root::{BILLBOARD_COLLECTION_FS, BILLBOARD_COLLECTION_VS, VIEWPORT_QUAD_VS},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Attribute locations for one layout (`attributeLocationsBatched` or
/// `attributeLocationsInstanced`).
#[derive(Clone, Copy, Debug)]
struct Locations {
    direction: Option<u32>,
    position_high_and_scale: u32,
    position_low_and_rotation: u32,
    compressed0: u32,
    compressed1: u32,
    compressed2: u32,
    eye_offset: u32,
    scale_by_distance: u32,
    pixel_offset_scale_by_distance: u32,
    compressed3: u32,
    texture_coordinate_bounds: u32,
    batch_id: u32,
    sdf: u32,
    split_direction: u32,
}

const BATCHED: Locations = Locations {
    direction: None,
    position_high_and_scale: 0,
    position_low_and_rotation: 1,
    compressed0: 2,
    compressed1: 3,
    compressed2: 4,
    eye_offset: 5,
    scale_by_distance: 6,
    pixel_offset_scale_by_distance: 7,
    compressed3: 8,
    texture_coordinate_bounds: 9,
    batch_id: 10,
    sdf: 11,
    split_direction: 12,
};

const INSTANCED: Locations = Locations {
    direction: Some(0),
    position_high_and_scale: 1,
    position_low_and_rotation: 2,
    compressed0: 3,
    compressed1: 4,
    compressed2: 5,
    eye_offset: 6,
    scale_by_distance: 7,
    pixel_offset_scale_by_distance: 8,
    compressed3: 9,
    texture_coordinate_bounds: 10,
    batch_id: 11,
    sdf: 12,
    split_direction: 13,
};

impl Locations {
    fn map(&self) -> BTreeMap<String, u32> {
        let mut m: BTreeMap<String, u32> = [
            ("positionHighAndScale", self.position_high_and_scale),
            ("positionLowAndRotation", self.position_low_and_rotation),
            ("compressedAttribute0", self.compressed0),
            ("compressedAttribute1", self.compressed1),
            ("compressedAttribute2", self.compressed2),
            ("eyeOffset", self.eye_offset),
            ("scaleByDistance", self.scale_by_distance),
            ("pixelOffsetScaleByDistance", self.pixel_offset_scale_by_distance),
            ("compressedAttribute3", self.compressed3),
            ("textureCoordinateBoundsOrLabelTranslate", self.texture_coordinate_bounds),
            ("a_batchId", self.batch_id),
            ("sdf", self.sdf),
            ("splitDirection", self.split_direction),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        if let Some(direction) = self.direction {
            m.insert("direction".into(), direction);
        }
        m
    }
}

const UPPER_BOUND: f64 = 32768.0;
const LEFT_SHIFT16: f64 = 65536.0;
const LEFT_SHIFT12: f64 = 4096.0;
const LEFT_SHIFT8: f64 = 256.0;
const LEFT_SHIFT7: f64 = 128.0;
const LEFT_SHIFT5: f64 = 32.0;
const LEFT_SHIFT3: f64 = 8.0;
const LEFT_SHIFT2: f64 = 4.0;
const RIGHT_SHIFT8: f64 = 1.0 / 256.0;
const LOWER_LEFT: f64 = 0.0;
const LOWER_RIGHT: f64 = 2.0;
const UPPER_RIGHT: f64 = 3.0;
const UPPER_LEFT: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct BillboardCollectionOptions {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub debug_show_texture_atlas: bool,
    pub blend_option: BlendOption,
    pub coarse_depth_test_distance: Option<f64>,
    pub three_point_depth_test_distance: Option<f64>,
}

impl Default for BillboardCollectionOptions {
    fn default() -> Self {
        Self {
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            debug_show_texture_atlas: false,
            blend_option: BlendOption::OpaqueAndTranslucent,
            coarse_depth_test_distance: None,
            three_point_depth_test_distance: None,
        }
    }
}

/// What the collection put in one draw command, as Cesium's `_colorCommands`.
#[derive(Clone, Debug, PartialEq)]
pub struct BillboardCommand {
    pub pass: Pass,
    pub primitive_type: PrimitiveType,
    pub vertex_array: usize,
    pub opaque_program: bool,
    pub render_state: RenderState,
    pub bounding_volume: BoundingSphere,
    pub model_matrix: Matrix4,
    pub pick_id: String,
    pub debug_show_bounding_volume: bool,
    pub count: f64,
    /// 0, DrawCommand's default, when not instanced.
    pub instance_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BillboardProgram {
    pub vertex_defines: Vec<String>,
    pub fragment_defines: Vec<String>,
}

struct Program<G: Gl> {
    program: Rc<ShaderProgram<G>>,
    sources: BillboardProgram,
}

/// The shader features the written billboards need (`_shader*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BillboardShaderFlags {
    pub rotation: bool,
    pub aligned_axis: bool,
    pub scale_by_distance: bool,
    pub translucency_by_distance: bool,
    pub pixel_offset_scale_by_distance: bool,
    pub distance_display_condition: bool,
    pub disable_depth_distance: bool,
    pub clamp_to_ground: bool,
    pub sdf: bool,
}

/// The extents `updateBoundingVolume` grows the volume by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BillboardExtents {
    pub max_size: f64,
    pub max_eye_offset: f64,
    pub max_scale: f64,
    pub max_pixel_offset: f64,
    pub all_horizontal_center: bool,
    pub all_vertical_center: bool,
    pub all_sized_in_meters: bool,
}

/// Per-update values the writers read.
struct WriteContext<G: Gl + 'static> {
    context: Context<G>,
    vertex_texture_reads: bool,
    depth_texture: bool,
}

type Writer<G> = fn(&mut BillboardCollection<G>, &WriteContext<G>, &Billboard<G>) -> Result<()>;

/// `_sp` and `_spTranslucent`.
pub type ShaderProgramPair<G> = (Option<Rc<ShaderProgram<G>>>, Option<Rc<ShaderProgram<G>>>);

pub struct BillboardCollection<G: Gl + 'static> {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub debug_show_texture_atlas: bool,
    pub blend_option: BlendOption,
    /// `_highlightColor`, used by vector tile points.
    pub highlight_color: Color,
    /// `coarseDepthTestDistance`.
    pub coarse_depth_test_distance: f64,
    /// `threePointDepthTestDistance`.
    pub three_point_depth_test_distance: f64,
    /// `destroyTextureAtlas`: destroy the atlas with the collection.
    pub destroy_texture_atlas: bool,
    shared: Rc<CollectionShared<G>>,
    billboards: Vec<Option<Billboard<G>>>,
    billboards_removed: bool,
    create_vertex_array: bool,
    atlas_guid: String,
    shader: BillboardShaderFlags,
    compiled_shader: Option<BillboardShaderFlags>,
    extents: BillboardExtents,
    base_volume: BoundingSphere,
    base_volume_wc: BoundingSphere,
    base_volume_2d: BoundingSphere,
    bounding_volume_dirty: bool,
    applied_model_matrix: Matrix4,
    applied_blend_option: Option<BlendOption>,
    mode: SceneMode,
    instanced: bool,
    buffers_usage: Vec<BufferUsage>,
    rs_opaque: Option<RenderState>,
    rs_translucent: Option<RenderState>,
    sp: Option<Program<G>>,
    sp_translucent: Option<Program<G>>,
    debug_program: Option<Rc<ShaderProgram<G>>>,
    vaf: Option<VertexArrayFacade<G>>,
    commands: Vec<BillboardCommand>,
    all_billboards_ready: bool,
    destroyed: Rc<Cell<bool>>,
}

impl<G: Gl + 'static> Default for BillboardCollection<G> {
    fn default() -> Self {
        Self::new(BillboardCollectionOptions::default())
    }
}

/// `context.cache.billboardCollection_*`: buffers shared by every collection
/// on a context.
fn cached_buffer<G: Gl + 'static>(
    context: &Context<G>,
    key: &'static str,
    create: impl FnOnce() -> Result<Buffer<G>>,
) -> Result<Buffer<G>> {
    context.cached_buffer(key, create)
}

fn index_buffer<G: Gl + 'static>(context: &Context<G>, instanced: bool) -> Result<Buffer<G>> {
    if instanced {
        return cached_buffer(context, "billboardCollection_indexBufferInstanced", || {
            let bytes: Vec<u8> = [0u16, 1, 2, 0, 2, 3].iter().flat_map(|v| v.to_le_bytes()).collect();
            Buffer::new(
                context,
                BufferKind::Index(IndexDatatype::UnsignedShort),
                BufferUsage::StaticDraw,
                BufferData::Bytes(&bytes),
            )
        });
    }
    cached_buffer(context, "billboardCollection_indexBufferBatched", || {
        let length = 16 * 1024 * 6 - 6;
        let mut indices = vec![0u16; length];
        let (mut i, mut j) = (0usize, 0u16);
        while i < length {
            indices[i..i + 6].copy_from_slice(&[j, j + 1, j + 2, j, j + 2, j + 3]);
            i += 6;
            j = j.wrapping_add(4);
        }
        let bytes: Vec<u8> = indices.iter().flat_map(|v| v.to_le_bytes()).collect();
        Buffer::new(
            context,
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&bytes),
        )
    })
}

fn vertex_buffer_instanced<G: Gl + 'static>(context: &Context<G>) -> Result<Buffer<G>> {
    cached_buffer(context, "billboardCollection_vertexBufferInstanced", || {
        let bytes: Vec<u8> = [0f32, 0., 1., 0., 1., 1., 0., 1.].iter().flat_map(|v| v.to_le_bytes()).collect();
        Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))
    })
}

impl<G: Gl + 'static> BillboardCollection<G> {
    pub fn new(options: BillboardCollectionOptions) -> Self {
        let atlas = TextureAtlas::new(TextureAtlasOptions::default()).expect("default atlas options are valid");
        Self::with_texture_atlas(options, atlas)
    }

    /// A collection drawing from `atlas` (Cesium's hidden `textureAtlas`
    /// option, used by labels).
    pub fn with_texture_atlas(options: BillboardCollectionOptions, atlas: TextureAtlas<G>) -> Self {
        let ellipsoid_minimum = crate::core::Ellipsoid::WGS84.minimum_radius;
        let atlas_guid = atlas.guid().to_string();
        let textures = Rc::new(TextureHost {
            atlas: RefCell::new(Rc::new(RefCell::new(atlas))),
            cache: RefCell::default(),
            pending: RefCell::default(),
        });
        Self {
            show: options.show,
            model_matrix: options.model_matrix,
            debug_show_bounding_volume: options.debug_show_bounding_volume,
            debug_show_texture_atlas: options.debug_show_texture_atlas,
            blend_option: options.blend_option,
            highlight_color: Color::WHITE,
            coarse_depth_test_distance: options.coarse_depth_test_distance.unwrap_or(ellipsoid_minimum / 10.0),
            three_point_depth_test_distance: options
                .three_point_depth_test_distance
                .unwrap_or(ellipsoid_minimum / 1000.0),
            destroy_texture_atlas: true,
            shared: Rc::new(CollectionShared {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                queue: RefCell::default(),
                textures,
            }),
            billboards: Vec::new(),
            billboards_removed: false,
            create_vertex_array: false,
            atlas_guid,
            shader: BillboardShaderFlags::default(),
            compiled_shader: None,
            extents: BillboardExtents {
                max_size: 0.0,
                max_eye_offset: 0.0,
                max_scale: 1.0,
                max_pixel_offset: 0.0,
                all_horizontal_center: true,
                all_vertical_center: true,
                all_sized_in_meters: true,
            },
            base_volume: BoundingSphere::default(),
            base_volume_wc: BoundingSphere::default(),
            base_volume_2d: BoundingSphere::default(),
            bounding_volume_dirty: false,
            applied_model_matrix: Matrix4::IDENTITY,
            applied_blend_option: None,
            mode: SceneMode::Scene3D,
            instanced: false,
            buffers_usage: vec![BufferUsage::StaticDraw; 17],
            rs_opaque: None,
            rs_translucent: None,
            sp: None,
            sp_translucent: None,
            debug_program: None,
            vaf: None,
            commands: Vec::new(),
            all_billboards_ready: false,
            destroyed: Rc::new(Cell::new(false)),
        }
    }

    fn check(&self) -> Result<()> {
        if self.destroyed.get() {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    /// The texture atlas the billboards' images are packed into.
    pub fn texture_atlas(&self) -> Rc<RefCell<TextureAtlas<G>>> {
        self.shared.textures.atlas()
    }

    /// Replaces the atlas, destroying the old one when
    /// `destroy_texture_atlas` is set.
    pub fn set_texture_atlas(&mut self, atlas: TextureAtlas<G>) -> Result<()> {
        if self.destroy_texture_atlas {
            self.texture_atlas().borrow_mut().destroy()?;
        }
        *self.shared.textures.atlas.borrow_mut() = Rc::new(RefCell::new(atlas));
        Ok(())
    }

    /// `sizeInBytes`: the atlas texture's size.
    pub fn size_in_bytes(&self) -> usize {
        self.texture_atlas().borrow().size_in_bytes()
    }

    /// `ready`: whether every shown billboard's image was loaded at the last
    /// complete update.
    pub fn ready(&self) -> bool {
        self.all_billboards_ready
    }

    /// `_sdf`: write and compile the signed-distance-field outline path, as
    /// `LabelCollection` does.
    pub fn set_sdf(&mut self, sdf: bool) {
        self.shader.sdf = sdf;
    }

    fn compact(&mut self) {
        if self.billboards_removed {
            self.billboards_removed = false;
            self.billboards.retain(Option::is_some);
            for (i, b) in self.billboards.iter().flatten().enumerate() {
                b.0.borrow_mut().index = i;
            }
        }
    }

    pub fn length(&mut self) -> usize {
        self.compact();
        self.billboards.len()
    }

    /// Adds a billboard, loading `image` into the atlas.
    pub fn add(&mut self, options: BillboardOptions, image: Option<BillboardImage<G>>) -> Result<Billboard<G>> {
        self.check()?;
        let billboard = Billboard::new(options, image, &self.shared, self.billboards.len())?;
        self.billboards.push(Some(billboard.clone()));
        self.create_vertex_array = true;
        Ok(billboard)
    }

    pub fn contains(&self, billboard: &Billboard<G>) -> bool {
        billboard.collection_id() == Some(self.shared.id)
    }

    pub fn remove(&mut self, billboard: &Billboard<G>) -> Result<bool> {
        self.check()?;
        if !self.contains(billboard) {
            return Ok(false);
        }
        let index = billboard.index();
        self.billboards[index] = None;
        self.billboards_removed = true;
        self.create_vertex_array = true;
        billboard.destroy();
        Ok(true)
    }

    pub fn remove_all(&mut self) -> Result<()> {
        self.check()?;
        for b in self.billboards.drain(..).flatten() {
            b.destroy();
        }
        self.shared.queue.borrow_mut().to_update.clear();
        self.billboards_removed = false;
        self.create_vertex_array = true;
        Ok(())
    }

    pub fn get(&mut self, index: usize) -> Option<Billboard<G>> {
        self.compact();
        self.billboards.get(index).cloned().flatten()
    }

    fn compute_new_buffers_usage(&mut self) -> bool {
        let changed = self.shared.queue.borrow().properties_changed;
        let mut usage_changed = false;
        for (k, count) in changed.iter().enumerate() {
            let usage = if *count == 0 { BufferUsage::StaticDraw } else { BufferUsage::StreamDraw };
            match self.buffers_usage.get_mut(k) {
                Some(current) => {
                    usage_changed |= *current != usage;
                    *current = usage;
                }
                None => {
                    usage_changed = true;
                    self.buffers_usage.push(usage);
                }
            }
        }
        usage_changed
    }

    fn locations(&self) -> &'static Locations {
        if self.instanced {
            &INSTANCED
        } else {
            &BATCHED
        }
    }

    fn create_vaf(&self, context: &Context<G>, count: usize) -> Result<VertexArrayFacade<G>> {
        let l = self.locations();
        let usage = |property: usize| self.buffers_usage.get(property).copied().unwrap_or(BufferUsage::StaticDraw);
        let attribute = |index, components, property: usize| {
            let mut a = FacadeAttribute::new(index, components);
            a.usage = usage(property);
            a
        };
        let mut attributes = vec![
            attribute(l.position_high_and_scale, 4, POSITION_INDEX),
            attribute(l.position_low_and_rotation, 4, POSITION_INDEX),
            attribute(l.compressed0, 4, PIXEL_OFFSET_INDEX),
            attribute(l.compressed1, 4, TRANSLUCENCY_BY_DISTANCE_INDEX),
            attribute(l.compressed2, 4, COLOR_INDEX),
            attribute(l.eye_offset, 4, EYE_OFFSET_INDEX),
            attribute(l.scale_by_distance, 4, SCALE_BY_DISTANCE_INDEX),
            attribute(l.pixel_offset_scale_by_distance, 4, PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX),
            attribute(l.compressed3, 4, DISTANCE_DISPLAY_CONDITION),
            attribute(l.texture_coordinate_bounds, 4, TEXTURE_COORDINATE_BOUNDS),
            attribute(l.split_direction, 1, SPLIT_DIRECTION_INDEX),
        ];
        if let Some(direction) = l.direction {
            let mut a = FacadeAttribute::new(direction, 2);
            a.datatype = ComponentDatatype::Float;
            a.vertex_buffer = Some(vertex_buffer_instanced(context)?);
            attributes.push(a);
        }
        if self.shader.sdf {
            attributes.push(attribute(l.sdf, 2, SDF_INDEX));
        }
        let size = if self.instanced { count } else { 4 * count };
        VertexArrayFacade::new(context, attributes, size, self.instanced)
    }

    /// Writes `values(k)` for the billboard's one instanced vertex, or for
    /// each of its four corners.
    fn write(&mut self, attribute: u32, billboard: &Billboard<G>, values: impl Fn(usize) -> Vec<f64>) -> Result<()> {
        let index = billboard.index();
        let instanced = self.instanced;
        let vaf = self.vaf.as_mut().ok_or_else(|| RuntimeError::new("No vertex array facade"))?;
        if instanced {
            vaf.write(attribute, index, &values(0))
        } else {
            for k in 0..4 {
                vaf.write(attribute, index * 4 + k, &values(k))?;
            }
            Ok(())
        }
    }

    fn write_position_scale_and_rotation(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let position = b.actual_position();
        if self.mode == SceneMode::Scene3D {
            self.base_volume = self.base_volume.expand(position);
            self.bounding_volume_dirty = true;
        }
        let encoded = EncodedCartesian3::from_cartesian(position);
        let (scale, rotation) = (b.scale(), b.rotation());
        if rotation != 0.0 {
            self.shader.rotation = true;
        }
        self.extents.max_scale = self.extents.max_scale.max(scale);
        let (high, low) = (encoded.high, encoded.low);
        let l = *self.locations();
        self.write(l.position_high_and_scale, b, |_| vec![high.x, high.y, high.z, scale])?;
        self.write(l.position_low_and_rotation, b, |_| vec![low.x, low.y, low.z, rotation])
    }

    fn texture_rectangle(b: &Billboard<G>) -> Result<BoundingRectangle> {
        if b.ready() {
            b.compute_texture_coordinates()
        } else {
            Ok(BoundingRectangle::default())
        }
    }

    fn write_compressed_attrib0(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let (pixel_offset, translate, horizontal, vertical, show) = {
            let s = b.0.borrow();
            (
                s.o.pixel_offset,
                s.translate,
                s.o.horizontal_origin,
                s.o.vertical_origin,
                s.o.show && s.cluster_show && s.o.color.alpha != 0.0,
            )
        };
        self.extents.max_pixel_offset = self
            .extents
            .max_pixel_offset
            .max((pixel_offset.x + translate.x).abs())
            .max((-pixel_offset.y + translate.y).abs());
        let vertical = if vertical == VerticalOrigin::Baseline { VerticalOrigin::Bottom } else { vertical };
        self.extents.all_horizontal_center &= horizontal == HorizontalOrigin::Center;
        self.extents.all_vertical_center &= vertical == VerticalOrigin::Center;
        let r = Self::texture_rectangle(b)?;
        let (left, bottom, right, top) = (r.x, r.y, r.x + r.width, r.y + r.height);
        let clamp = |v: f64| math::clamp(v, -UPPER_BOUND, UPPER_BOUND);
        let mut compressed0 = (clamp(pixel_offset.x) + UPPER_BOUND).floor() * LEFT_SHIFT7;
        compressed0 += (horizontal.value() as f64 + 1.0) * LEFT_SHIFT5;
        compressed0 += (vertical.value() as f64 + 1.0) * LEFT_SHIFT3;
        compressed0 += if show { 1.0 } else { 0.0 } * LEFT_SHIFT2;
        let mut compressed1 = (clamp(pixel_offset.y) + UPPER_BOUND).floor() * LEFT_SHIFT8;
        let mut compressed2 = (clamp(translate.x * LEFT_SHIFT2) + UPPER_BOUND).floor() * LEFT_SHIFT8;
        let temp = (clamp(translate.y * LEFT_SHIFT2) + UPPER_BOUND) * RIGHT_SHIFT8;
        let upper = temp.floor();
        let lower = ((temp - upper) * LEFT_SHIFT8).floor();
        compressed1 += upper;
        compressed2 += lower;
        let tc = |x, y| AttributeCompression::compress_texture_coordinates(Cartesian2::new(x, y));
        let corners = [
            (LOWER_LEFT, tc(left, bottom)),
            (LOWER_RIGHT, tc(right, bottom)),
            (UPPER_RIGHT, tc(right, top)),
            (UPPER_LEFT, tc(left, top)),
        ];
        let instanced = self.instanced;
        let l = *self.locations();
        self.write(l.compressed0, b, |k| {
            let (corner, coords) = corners[k];
            vec![compressed0 + if instanced { 0.0 } else { corner }, compressed1, compressed2, coords]
        })
    }

    fn write_compressed_attrib1(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let (axis, translucency) = (b.aligned_axis(), b.translucency_by_distance());
        if axis != Cartesian3::ZERO {
            self.shader.aligned_axis = true;
        }
        let (mut near, mut near_value, mut far, mut far_value) = (0.0, 1.0, 1.0, 1.0);
        if let Some(t) = translucency {
            (near, near_value, far, far_value) = (t.near, t.near_value, t.far, t.far_value);
            if near_value != 1.0 || far_value != 1.0 {
                self.shader.translucency_by_distance = true;
            }
        }
        let image_width = math::js_round(b.width().unwrap_or(0.0));
        self.extents.max_size = self.extents.max_size.max(image_width);
        let mut compressed0 = math::clamp(image_width, 0.0, LEFT_SHIFT16);
        let mut compressed1 = 0.0;
        if (axis.magnitude_squared() - 1.0).abs() < math::EPSILON6 {
            compressed1 = AttributeCompression::oct_encode_float(axis);
        }
        let quantize = |v: f64| {
            let v = math::clamp(v, 0.0, 1.0);
            if v == 1.0 {
                255.0
            } else {
                math::to_int32(v * 255.0) as f64
            }
        };
        compressed0 = compressed0 * LEFT_SHIFT8 + quantize(near_value);
        compressed1 = compressed1 * LEFT_SHIFT8 + quantize(far_value);
        let l = *self.locations();
        self.write(l.compressed1, b, |_| vec![compressed0, compressed1, near, far])
    }

    fn write_compressed_attrib2(&mut self, w: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let color = b.color();
        let pick = b.get_pick_id(&w.context)?;
        let size_in_meters = if b.size_in_meters() { 1.0 } else { 0.0 };
        let valid_axis = if (b.aligned_axis().magnitude_squared() - 1.0).abs() < math::EPSILON6 { 1.0 } else { 0.0 };
        self.extents.all_sized_in_meters &= size_in_meters == 1.0;
        let image_height = b.height().unwrap_or(0.0);
        self.extents.max_size = self.extents.max_size.max(image_height);
        let label_origin = b.0.borrow().label_horizontal_origin.map_or(-2.0, |o| o.value() as f64) + 2.0;
        let compressed0 = AttributeCompression::encode_rgb8(color);
        let compressed1 = AttributeCompression::encode_rgb8(pick);
        let compressed2 = Color::float_to_byte(color.alpha) as f64 * LEFT_SHIFT16
            + Color::float_to_byte(pick.alpha) as f64 * LEFT_SHIFT8
            + (size_in_meters * 2.0 + valid_axis);
        let compressed3 = image_height * LEFT_SHIFT2 + label_origin;
        let l = *self.locations();
        self.write(l.compressed2, b, |_| vec![compressed0, compressed1, compressed2, compressed3])
    }

    fn write_eye_offset(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let eye = b.eye_offset();
        let mut z = eye.z;
        if b.height_reference() != super::HeightReference::None {
            z *= 1.005;
        }
        self.extents.max_eye_offset = self.extents.max_eye_offset.max(eye.x.abs()).max(eye.y.abs()).max(z.abs());
        let range = if self.instanced {
            let r = Self::texture_rectangle(b)?;
            AttributeCompression::compress_texture_coordinates(Cartesian2::new(r.width, r.height))
        } else {
            0.0
        };
        let l = *self.locations();
        self.write(l.eye_offset, b, |_| vec![eye.x, eye.y, z, range])
    }

    fn near_far(
        &mut self,
        value: Option<crate::core::NearFarScalar>,
        flag: fn(&mut BillboardShaderFlags) -> &mut bool,
    ) -> [f64; 4] {
        match value {
            Some(v) => {
                if v.near_value != 1.0 || v.far_value != 1.0 {
                    *flag(&mut self.shader) = true;
                }
                [v.near, v.near_value, v.far, v.far_value]
            }
            None => [0.0, 1.0, 1.0, 1.0],
        }
    }

    fn write_scale_by_distance(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let v = self.near_far(b.scale_by_distance(), |f| &mut f.scale_by_distance);
        let l = *self.locations();
        self.write(l.scale_by_distance, b, |_| v.to_vec())
    }

    fn write_pixel_offset_scale_by_distance(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let v = self.near_far(b.pixel_offset_scale_by_distance(), |f| &mut f.pixel_offset_scale_by_distance);
        let l = *self.locations();
        self.write(l.pixel_offset_scale_by_distance, b, |_| v.to_vec())
    }

    fn write_compressed_attribute3(&mut self, w: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let (mut near, mut far) = (0.0, f64::MAX);
        if let Some(d) = b.distance_display_condition() {
            (near, far) = (d.near * d.near, d.far * d.far);
            self.shader.distance_display_condition = true;
        }
        // `undefined * undefined` is NaN, which is neither > 0 nor Infinity.
        let mut disable = b.disable_depth_test_distance().map_or(f64::NAN, |d| d * d);
        let clamp_to_ground = is_height_reference_clamp(b.height_reference()) && w.depth_texture;
        if clamp_to_ground || disable > 0.0 {
            self.shader.disable_depth_distance = true;
            if disable == f64::INFINITY {
                disable = -1.0;
            }
        }
        let (width, height) = match b.0.borrow().label_dimensions {
            Some(d) => (d.x, d.y),
            None => (b.width().unwrap_or(0.0), b.height().unwrap_or(0.0)),
        };
        let w12 = math::clamp(width, 0.0, LEFT_SHIFT12).floor();
        let h12 = math::clamp(height, 0.0, LEFT_SHIFT12).floor();
        let dimensions = w12 * LEFT_SHIFT12 + h12;
        let l = *self.locations();
        self.write(l.compressed3, b, |_| vec![near, far, disable, dimensions])
    }

    fn write_texture_coordinate_bounds_or_label_translate(
        &mut self,
        w: &WriteContext<G>,
        b: &Billboard<G>,
    ) -> Result<()> {
        if is_height_reference_clamp(b.height_reference()) {
            // Clamping needs a scene; without one there is no globe to test
            // depth against.
            self.shader.clamp_to_ground = false;
        }
        let l = *self.locations();
        if w.vertex_texture_reads {
            let t = b.0.borrow().label_translate.unwrap_or(Cartesian2::ZERO);
            return self.write(l.texture_coordinate_bounds, b, |_| vec![t.x, t.y, 0.0, 0.0]);
        }
        let r = Self::texture_rectangle(b)?;
        self.write(l.texture_coordinate_bounds, b, |_| vec![r.x, r.y, r.x + r.width, r.y + r.height])
    }

    fn write_sdf(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        if !self.shader.sdf {
            return Ok(());
        }
        let (color, width) = (b.outline_color(), b.outline_width());
        let compressed0 = AttributeCompression::encode_rgb8(color);
        let distance = width / sdf_settings::RADIUS;
        let compressed1 = Color::float_to_byte(color.alpha) as f64 * LEFT_SHIFT16
            + Color::float_to_byte(distance) as f64 * LEFT_SHIFT8;
        let instanced = self.instanced;
        let corners = [LOWER_LEFT, LOWER_RIGHT, UPPER_RIGHT, UPPER_LEFT];
        let l = *self.locations();
        self.write(l.sdf, b, |k| vec![compressed0 + if instanced { 0.0 } else { corners[k] }, compressed1])
    }

    fn write_split_direction(&mut self, _: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        let direction = b.split_direction().value() as f64;
        let l = *self.locations();
        self.write(l.split_direction, b, |_| vec![direction])
    }

    fn write_billboard(&mut self, w: &WriteContext<G>, b: &Billboard<G>) -> Result<()> {
        self.write_position_scale_and_rotation(w, b)?;
        self.write_compressed_attrib0(w, b)?;
        self.write_compressed_attrib1(w, b)?;
        self.write_compressed_attrib2(w, b)?;
        self.write_eye_offset(w, b)?;
        self.write_scale_by_distance(w, b)?;
        self.write_pixel_offset_scale_by_distance(w, b)?;
        self.write_compressed_attribute3(w, b)?;
        self.write_texture_coordinate_bounds_or_label_translate(w, b)?;
        self.write_sdf(w, b)?;
        self.write_split_direction(w, b)
    }

    fn recompute_actual_positions(&mut self, frame: &FrameState, billboards: &[Billboard<G>], recompute_volume: bool) {
        let in_3d = frame.mode == SceneMode::Scene3D;
        if in_3d {
            self.bounding_volume_dirty = true;
        }
        let state = SceneTransformState::from_frame(frame, Cartesian2::ZERO);
        let mut positions = Vec::new();
        for b in billboards {
            let actual = match b.0.borrow().clamped_position {
                Some(clamped) => Some(clamped),
                None => Billboard::<G>::compute_actual_position(b.position(), &state, &self.applied_model_matrix),
            };
            if let Some(actual) = actual {
                b.set_actual_position(actual);
                if recompute_volume {
                    positions.push(actual);
                } else if in_3d {
                    self.base_volume = self.base_volume.expand(actual);
                } else {
                    self.base_volume_2d = self.base_volume_2d.expand(actual);
                }
            }
        }
        if recompute_volume {
            let volume = BoundingSphere::from_points(&positions);
            if in_3d {
                self.base_volume = volume;
            } else {
                self.base_volume_2d = volume;
            }
        }
    }

    fn update_mode(&mut self, frame: &FrameState) {
        let mode = frame.mode;
        let billboards: Vec<Billboard<G>> = self.billboards.iter().flatten().cloned().collect();
        if self.create_vertex_array
            || self.mode != mode
            || (mode != SceneMode::Scene3D && self.applied_model_matrix != self.model_matrix)
        {
            self.mode = mode;
            self.applied_model_matrix = self.model_matrix;
            self.create_vertex_array = true;
            if matches!(mode, SceneMode::Scene3D | SceneMode::Scene2D | SceneMode::ColumbusView) {
                self.recompute_actual_positions(frame, &billboards, true);
            }
        } else if mode == SceneMode::Morphing {
            self.recompute_actual_positions(frame, &billboards, true);
        } else if matches!(mode, SceneMode::Scene2D | SceneMode::ColumbusView) {
            let queued = self.shared.queue.borrow().to_update.clone();
            self.recompute_actual_positions(frame, &queued, false);
        }
    }

    fn grow_bounding_volume(
        &self,
        frame: &FrameState,
        context: &Context<G>,
        volume: &mut BoundingSphere,
    ) -> Result<()> {
        let e = self.extents;
        let mut pixel_scale = 1.0;
        if !e.all_sized_in_meters || e.max_pixel_offset != 0.0 {
            let camera = &frame.uniform.camera;
            let to_center = camera.position - volume.center;
            let projection = camera.direction.multiply_by_scalar(to_center.dot(camera.direction));
            let distance = (projection.magnitude() - volume.radius).max(0.0);
            let [width, height] = context.drawing_buffer_size()?;
            let (width, height, ratio) = (width as f64, height as f64, frame.pixel_ratio);
            let pixel = match &camera.frustum {
                UniformFrustum::Perspective(f) => f.get_pixel_dimensions(width, height, distance, ratio),
                UniformFrustum::PerspectiveOffCenter(f) => f.get_pixel_dimensions(width, height, distance, ratio),
                UniformFrustum::Orthographic(f) => f.get_pixel_dimensions(width, height, distance, ratio),
                UniformFrustum::OrthographicOffCenter(f) => f.get_pixel_dimensions(width, height, distance, ratio),
            };
            pixel_scale = pixel.x.max(pixel.y);
        }
        let mut size = pixel_scale * e.max_scale * e.max_size * 2.0;
        if e.all_horizontal_center && e.all_vertical_center {
            size *= 0.5;
        }
        let offset = pixel_scale * e.max_pixel_offset + e.max_eye_offset;
        volume.radius += size + offset;
        Ok(())
    }

    fn compile(&mut self, context: &Context<G>, vertex_texture_reads: bool) -> Result<()> {
        let s = self.shader;
        let mut vertex = ShaderSource::new(BILLBOARD_COLLECTION_VS.source);
        for (enabled, define) in [
            (self.instanced, "INSTANCED"),
            (s.rotation, "ROTATION"),
            (s.aligned_axis, "ALIGNED_AXIS"),
            (s.scale_by_distance, "EYE_DISTANCE_SCALING"),
            (s.translucency_by_distance, "EYE_DISTANCE_TRANSLUCENCY"),
            (s.pixel_offset_scale_by_distance, "EYE_DISTANCE_PIXEL_OFFSET"),
            (s.distance_display_condition, "DISTANCE_DISPLAY_CONDITION"),
            (s.disable_depth_distance, "DISABLE_DEPTH_DISTANCE"),
        ] {
            if enabled {
                vertex.defines.push(define.to_string());
            }
        }
        let depth_check =
            if vertex_texture_reads { "VS_THREE_POINT_DEPTH_CHECK" } else { "FS_THREE_POINT_DEPTH_CHECK" };
        if s.clamp_to_ground {
            vertex.defines.push(depth_check.to_string());
        }
        if s.sdf {
            vertex.defines.push("SDF".to_string());
        }
        let sdf_edge = math::js_number_to_string(1.0 - sdf_settings::CUTOFF);
        let locations = self.locations().map();
        let build = |previous: Option<Program<G>>, first: Option<&str>| -> Result<Program<G>> {
            let mut fragment = ShaderSource::new(BILLBOARD_COLLECTION_FS.source);
            fragment.defines = first.into_iter().map(String::from).collect();
            // The vector-tile define, empty without a batch table.
            fragment.defines.push(String::new());
            if s.clamp_to_ground {
                fragment.defines.push(depth_check.to_string());
            }
            if s.sdf {
                fragment.defines.push("SDF".to_string());
                fragment.defines.push(format!("SDF_EDGE {sdf_edge}"));
            }
            let options = ShaderProgramOptions {
                vertex_shader_source: vertex.clone(),
                fragment_shader_source: fragment.clone(),
                attribute_locations: locations.clone(),
            };
            let program =
                context.with_shader_cache(|c| c.replace_shader_program(previous.map(|p| p.program), &options))?;
            Ok(Program {
                program,
                sources: BillboardProgram {
                    vertex_defines: vertex.defines.clone(),
                    fragment_defines: fragment.defines,
                },
            })
        };
        match self.applied_blend_option {
            Some(BlendOption::OpaqueAndTranslucent) => {
                self.sp = Some(build(self.sp.take(), Some("OPAQUE"))?);
                self.sp_translucent = Some(build(self.sp_translucent.take(), Some("TRANSLUCENT"))?);
            }
            Some(BlendOption::Opaque) => self.sp = Some(build(self.sp.take(), None)?),
            Some(BlendOption::Translucent) => self.sp_translucent = Some(build(self.sp_translucent.take(), None)?),
            None => {}
        }
        self.compiled_shader = Some(self.shader);
        Ok(())
    }

    /// Pushes the frame's `afterRender` atlas update, which also settles the
    /// image loads it resolves.
    fn schedule_atlas_update(&self, frame: &mut FrameState, context: &Context<G>) {
        let destroyed = self.destroyed.clone();
        let textures = Rc::downgrade(&self.shared.textures);
        let context = context.clone();
        frame.after_render.push(Box::new(move || {
            if destroyed.get() {
                return Ok(false);
            }
            let Some(textures) = textures.upgrade() else { return Ok(false) };
            let atlas = textures.atlas();
            let updated = atlas.borrow_mut().update(&context)?;
            poll_pending(&textures);
            Ok(updated)
        }));
    }

    fn update_frame(&mut self, frame: &mut FrameState) -> Result<()> {
        self.check()?;
        self.compact();
        self.commands.clear();
        if !self.show {
            return Ok(());
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|c| c.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("frameState.context is required."))?;
        let capabilities = context.capabilities()?;
        let limits = context.limits()?;
        self.instanced = capabilities.instanced_arrays;
        let w = WriteContext {
            context: context.clone(),
            vertex_texture_reads: limits.maximum_vertex_texture_image_units > 0,
            depth_texture: capabilities.depth_texture,
        };

        poll_pending(&self.shared.textures);
        let mut all_ready = true;
        let billboards: Vec<Billboard<G>> = self.billboards.iter().flatten().cloned().collect();
        for b in &billboards {
            if b.load_error().is_some() {
                b.set_image(None);
            }
            if b.texture_dirty() {
                self.shared.update_billboard(b, IMAGE_INDEX_INDEX);
            }
            if b.show() {
                all_ready &= b.ready();
            }
        }

        self.schedule_atlas_update(frame, &context);
        let atlas = self.texture_atlas();
        let Some(atlas_texture) = atlas.borrow().texture().cloned() else { return Ok(()) };

        self.update_mode(frame);

        let to_update = self.shared.queue.borrow().to_update.clone();
        let guid = atlas.borrow().guid().to_string();
        let create = self.create_vertex_array || self.atlas_guid != guid;
        self.atlas_guid = guid;
        let picking = frame.passes.pick;

        if create || (!picking && self.compute_new_buffers_usage()) {
            self.create_vertex_array = false;
            self.shared.queue.borrow_mut().properties_changed = [0; NUMBER_OF_PROPERTIES];
            if let Some(mut vaf) = self.vaf.take() {
                vaf.destroy()?;
            }
            if !billboards.is_empty() {
                self.vaf = Some(self.create_vaf(&context, billboards.len())?);
                for b in &billboards {
                    b.0.borrow_mut().dirty = false;
                    b.set_texture_dirty(false);
                    self.write_billboard(&w, b)?;
                }
                let index = index_buffer(&context, self.instanced)?;
                self.vaf.as_mut().unwrap().commit(Some(index))?;
            }
            self.shared.queue.borrow_mut().to_update.clear();
        } else if !to_update.is_empty() {
            let p = self.shared.queue.borrow().properties_changed;
            let any = |indices: &[usize]| indices.iter().any(|&i| p[i] > 0);
            let mut writers: Vec<Writer<G>> = Vec::new();
            if any(&[POSITION_INDEX, ROTATION_INDEX, SCALE_INDEX]) {
                writers.push(Self::write_position_scale_and_rotation);
            }
            if any(&[IMAGE_INDEX_INDEX, PIXEL_OFFSET_INDEX, HORIZONTAL_ORIGIN_INDEX, VERTICAL_ORIGIN_INDEX, SHOW_INDEX])
            {
                writers.push(Self::write_compressed_attrib0);
                if self.instanced {
                    writers.push(Self::write_eye_offset);
                }
            }
            if any(&[IMAGE_INDEX_INDEX, ALIGNED_AXIS_INDEX, TRANSLUCENCY_BY_DISTANCE_INDEX]) {
                writers.push(Self::write_compressed_attrib1);
                writers.push(Self::write_compressed_attrib2);
            }
            if any(&[IMAGE_INDEX_INDEX, COLOR_INDEX]) {
                writers.push(Self::write_compressed_attrib2);
            }
            if any(&[IMAGE_INDEX_INDEX, EYE_OFFSET_INDEX]) {
                writers.push(Self::write_eye_offset);
            }
            if any(&[SCALE_BY_DISTANCE_INDEX]) {
                writers.push(Self::write_scale_by_distance);
            }
            if any(&[PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX]) {
                writers.push(Self::write_pixel_offset_scale_by_distance);
            }
            if any(&[DISTANCE_DISPLAY_CONDITION, DISABLE_DEPTH_DISTANCE, IMAGE_INDEX_INDEX, POSITION_INDEX]) {
                writers.push(Self::write_compressed_attribute3);
            }
            if any(&[IMAGE_INDEX_INDEX, POSITION_INDEX]) {
                writers.push(Self::write_texture_coordinate_bounds_or_label_translate);
            }
            if any(&[SDF_INDEX]) {
                writers.push(Self::write_sdf);
            }
            if any(&[SPLIT_DIRECTION_INDEX]) {
                writers.push(Self::write_split_direction);
            }
            let rewrite_all = to_update.len() as f64 / billboards.len() as f64 > 0.1;
            for b in &to_update {
                b.0.borrow_mut().dirty = false;
                b.set_texture_dirty(false);
                for writer in &writers {
                    writer(self, &w, b)?;
                }
                if !rewrite_all {
                    let (offset, length) = if self.instanced { (b.index(), 1) } else { (b.index() * 4, 4) };
                    self.vaf.as_ref().unwrap().sub_commit(offset, length)?;
                }
            }
            if rewrite_all {
                let index = index_buffer(&context, self.instanced)?;
                self.vaf.as_mut().unwrap().commit(Some(index))?;
            } else {
                self.vaf.as_mut().unwrap().end_sub_commits();
            }
            self.shared.queue.borrow_mut().to_update.clear();
        }

        let Some(arrays) = self.vaf.as_ref().and_then(|v| v.vertex_arrays()) else { return Ok(()) };
        let arrays: Vec<_> = arrays.iter().map(|a| (a.vertex_array.clone(), a.indices_count)).collect();

        if self.bounding_volume_dirty {
            self.bounding_volume_dirty = false;
            self.base_volume_wc = self.base_volume.transform(&self.model_matrix);
        }
        let (model_matrix, mut bounding_volume) = if frame.mode == SceneMode::Scene3D {
            (self.model_matrix, self.base_volume_wc)
        } else {
            (Matrix4::IDENTITY, self.base_volume_2d)
        };
        self.grow_bounding_volume(frame, &context, &mut bounding_volume)?;

        let blend_option_changed = self.applied_blend_option != Some(self.blend_option);
        self.applied_blend_option = Some(self.blend_option);
        if blend_option_changed {
            self.rs_opaque =
                matches!(self.blend_option, BlendOption::Opaque | BlendOption::OpaqueAndTranslucent).then(|| {
                    RenderState {
                        depth_test: DepthTest { enabled: true, func: gl::LESS },
                        depth_mask: true,
                        ..Default::default()
                    }
                });
            let translucent_depth_mask = self.blend_option == BlendOption::Translucent;
            self.rs_translucent =
                matches!(self.blend_option, BlendOption::Translucent | BlendOption::OpaqueAndTranslucent).then(|| {
                    RenderState {
                        depth_test: DepthTest {
                            enabled: true,
                            func: if translucent_depth_mask { gl::LEQUAL } else { gl::LESS },
                        },
                        depth_mask: translucent_depth_mask,
                        blending: blending_state::alpha_blend(),
                        ..Default::default()
                    }
                });
        }

        self.shader.disable_depth_distance |= frame.minimum_disable_depth_test_distance.unwrap_or(0.0) != 0.0;
        if blend_option_changed || self.compiled_shader != Some(self.shader) {
            self.compile(&context, w.vertex_texture_reads)?;
        }

        if frame.passes.render || picking {
            let opaque = self.blend_option == BlendOption::Opaque;
            let both = self.blend_option == BlendOption::OpaqueAndTranslucent;
            let total = if both { arrays.len() * 2 } else { arrays.len() };
            let count = billboards.len();
            for j in 0..total {
                let opaque_command = opaque || (both && j % 2 == 0);
                let index = if both { j / 2 } else { j };
                let pass = if opaque_command || !both { Pass::Opaque } else { Pass::Translucent };
                let (program, state) = if opaque_command {
                    (self.sp.as_ref(), self.rs_opaque.clone())
                } else {
                    (self.sp_translucent.as_ref(), self.rs_translucent.clone())
                };
                let program = program.ok_or_else(|| RuntimeError::new("Billboard shader program is not compiled"))?;
                let state = state.ok_or_else(|| RuntimeError::new("Billboard render state is not created"))?;
                let (vertex_array, indices_count) = arrays[index].clone();
                let (draw_count, instance_count) = if self.instanced { (6.0, count) } else { (indices_count, 0) };
                let command = BillboardCommand {
                    pass,
                    primitive_type: PrimitiveType::Triangles,
                    vertex_array: index,
                    opaque_program: opaque_command,
                    render_state: state.clone(),
                    bounding_volume,
                    model_matrix,
                    pick_id: "v_pickColor".into(),
                    debug_show_bounding_volume: self.debug_show_bounding_volume,
                    count: draw_count,
                    instance_count,
                };
                let metadata = DrawMetadata {
                    bounding_volume: Some(bounding_volume),
                    model_matrix: Some(model_matrix),
                    owner: Some(format!("BillboardCollection#{}", self.shared.id)),
                    pick_id: Some(command.pick_id.clone()),
                    debug_show_bounding_volume: self.debug_show_bounding_volume,
                    ..Default::default()
                };
                let mut options = SceneDrawOptions::new(vertex_array, program.program.clone());
                options.state = state;
                options.primitive_type = PrimitiveType::Triangles;
                options.count = Some(draw_count as usize);
                options.instance_count = instance_count as u32;
                options.textures.push(("u_atlas".into(), atlas_texture.clone() as Rc<dyn SampledTexture<G>>));
                let h = self.highlight_color;
                options.uniforms = vec![
                    (
                        "u_highlightColor".into(),
                        UniformValue::Float(vec![h.red as f32, h.green as f32, h.blue as f32, h.alpha as f32]),
                    ),
                    (
                        "u_coarseDepthTestDistance".into(),
                        UniformValue::Float(vec![self.coarse_depth_test_distance as f32]),
                    ),
                    (
                        "u_threePointDepthTestDistance".into(),
                        UniformValue::Float(vec![self.three_point_depth_test_distance as f32]),
                    ),
                ];
                frame.command_list.push(SceneCommand::from_draw(
                    format!("billboard-collection-{}-{j}", self.shared.id),
                    context.clone(),
                    pass,
                    metadata,
                    options,
                ));
                self.commands.push(command);
            }
            if self.debug_show_texture_atlas {
                frame.command_list.push(self.debug_command(&context, &atlas_texture)?);
            }
        }
        self.all_billboards_ready = all_ready;
        Ok(())
    }

    /// `createDebugCommand`: an overlay quad showing the atlas.
    fn debug_command(&mut self, context: &Context<G>, atlas: &Rc<crate::renderer::Texture<G>>) -> Result<SceneCommand> {
        if self.debug_program.is_none() {
            let fs = "uniform sampler2D billboard_texture; \nin vec2 v_textureCoordinates; \nvoid main() \n{ \n    out_FragColor = texture(billboard_texture, v_textureCoordinates); \n} \n";
            let options = ShaderProgramOptions {
                vertex_shader_source: ShaderSource::new(VIEWPORT_QUAD_VS.source),
                fragment_shader_source: ShaderSource::new(fs),
                attribute_locations: [("position".to_string(), 0), ("textureCoordinates".to_string(), 1)].into(),
            };
            self.debug_program = Some(context.with_shader_cache(|c| c.get_shader_program(&options))?);
        }
        let mut options =
            SceneDrawOptions::new(context.get_viewport_quad_vertex_array()?, self.debug_program.clone().unwrap());
        options.textures.push(("billboard_texture".into(), atlas.clone() as Rc<dyn SampledTexture<G>>));
        Ok(SceneCommand::from_draw(
            format!("billboard-collection-{}-atlas", self.shared.id),
            context.clone(),
            Pass::Overlay,
            DrawMetadata::default(),
            options,
        ))
    }

    /// The commands queued by the last update.
    pub fn commands(&self) -> &[BillboardCommand] {
        &self.commands
    }
    pub fn vertex_array_facade(&self) -> Option<&VertexArrayFacade<G>> {
        self.vaf.as_ref()
    }
    pub fn programs(&self) -> (Option<&BillboardProgram>, Option<&BillboardProgram>) {
        (self.sp.as_ref().map(|p| &p.sources), self.sp_translucent.as_ref().map(|p| &p.sources))
    }
    pub fn shader_programs(&self) -> ShaderProgramPair<G> {
        (self.sp.as_ref().map(|p| p.program.clone()), self.sp_translucent.as_ref().map(|p| p.program.clone()))
    }
    pub fn shader_flags(&self) -> BillboardShaderFlags {
        self.shader
    }
    pub fn extents(&self) -> BillboardExtents {
        self.extents
    }
    pub fn buffers_usage(&self) -> &[BufferUsage] {
        &self.buffers_usage
    }
    pub fn properties_changed(&self) -> [u32; NUMBER_OF_PROPERTIES] {
        self.shared.queue.borrow().properties_changed
    }
    pub fn billboards_to_update(&self) -> usize {
        self.shared.queue.borrow().to_update.len()
    }
    /// Whether the last update drew instanced billboards.
    pub fn instanced(&self) -> bool {
        self.instanced
    }

    /// `computeScreenSpacePosition` for one of this collection's billboards.
    pub fn compute_screen_space_position(
        &self,
        billboard: &Billboard<G>,
        state: &SceneTransformState<'_>,
        camera: &mut super::Camera,
    ) -> Result<Option<Cartesian2>> {
        billboard.compute_screen_space_position(&self.model_matrix, state, camera)
    }
}

impl<G: Gl + 'static> ScenePrimitive for BillboardCollection<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.update_frame(frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<()> {
        Ok(())
    }
    /// Destroys the atlas (with `destroy_texture_atlas`), programs and
    /// vertex arrays and releases every billboard.
    fn destroy(&mut self) -> Result<()> {
        self.check()?;
        if self.destroy_texture_atlas {
            self.texture_atlas().borrow_mut().destroy()?;
        }
        self.sp = None;
        self.sp_translucent = None;
        if let Some(mut vaf) = self.vaf.take() {
            vaf.destroy()?;
        }
        for b in self.billboards.iter().flatten() {
            b.destroy();
        }
        self.commands.clear();
        self.destroyed.set(true);
        Ok(())
    }
}
