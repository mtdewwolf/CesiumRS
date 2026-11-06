//! CesiumJS `Billboard`: one billboard of a
//! [`BillboardCollection`](super::billboard_collection::BillboardCollection).
//!
//! A [`Billboard`] is a shared handle, as Cesium's billboard is a mutable
//! object the application keeps after `add`. Setters compare the old and new
//! values and, on a change, mark the billboard dirty in its collection as
//! Cesium's `makeDirty` does. Images load through the collection's texture
//! atlas ([`BillboardTexture`]).
//!
//! Height references other than `NONE` need a scene to clamp against; as in
//! Cesium without a scene, they are refused with "Height reference is not
//! supported without a scene."
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use futures_util::{future::LocalBoxFuture, FutureExt};

use super::billboard_texture::{source_with_size, BillboardTexture, TextureHost};
use crate::{
    core::{
        create_guid::create_guid, BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, Matrix4,
        NearFarScalar, RuntimeError,
    },
    renderer::{AtlasImageRequest, Context, Gl, PickId, TextureAtlas, TextureSource},
    scene::{
        scene_transforms::{SceneTransformState, SceneTransforms},
        BillboardLoadState, Camera, HeightReference, HorizontalOrigin, MetadataValue, SceneMode, ScenePickObject,
        SplitDirection, VerticalOrigin,
    },
};

pub const SHOW_INDEX: usize = 0;
pub const POSITION_INDEX: usize = 1;
pub const PIXEL_OFFSET_INDEX: usize = 2;
pub const EYE_OFFSET_INDEX: usize = 3;
pub const HORIZONTAL_ORIGIN_INDEX: usize = 4;
pub const VERTICAL_ORIGIN_INDEX: usize = 5;
pub const SCALE_INDEX: usize = 6;
pub const IMAGE_INDEX_INDEX: usize = 7;
pub const COLOR_INDEX: usize = 8;
pub const ROTATION_INDEX: usize = 9;
pub const ALIGNED_AXIS_INDEX: usize = 10;
pub const SCALE_BY_DISTANCE_INDEX: usize = 11;
pub const TRANSLUCENCY_BY_DISTANCE_INDEX: usize = 12;
pub const PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX: usize = 13;
pub const DISTANCE_DISPLAY_CONDITION: usize = 14;
pub const DISABLE_DEPTH_DISTANCE: usize = 15;
pub const TEXTURE_COORDINATE_BOUNDS: usize = 16;
pub const SDF_INDEX: usize = 17;
pub const SPLIT_DIRECTION_INDEX: usize = 18;
pub const NUMBER_OF_PROPERTIES: usize = 19;

/// SVG images with a size in pixels are rasterized at most this large.
const SVG_MAX_SIZE_PX: f64 = 512.0;

/// A billboard image: pixels or a host image ready now, or a future that
/// resolves to one (Cesium's URL, `Resource` and callback images). `uri` names
/// the image when no id is given and decides SVG sizing.
pub enum BillboardImage<G: Gl + 'static> {
    Source(TextureSource<G>),
    Future { uri: Option<String>, future: LocalBoxFuture<'static, Result<TextureSource<G>, RuntimeError>> },
}

impl<G: Gl + 'static> BillboardImage<G> {
    pub fn pixels(width: u32, height: u32, bytes: Vec<u8>) -> Self {
        Self::Source(TextureSource::Pixels { width, height, bytes })
    }
    fn uri(&self) -> Option<&str> {
        match self {
            Self::Source(_) => None,
            Self::Future { uri, .. } => uri.as_deref(),
        }
    }
    pub(crate) fn add_to(
        self,
        atlas: &mut TextureAtlas<G>,
        id: String,
        size: (Option<f64>, Option<f64>),
    ) -> Result<AtlasImageRequest, RuntimeError> {
        match self {
            Self::Source(source) => atlas.add_image_future(id, std::future::ready(Ok(source_with_size(source, size)))),
            Self::Future { future, .. } => {
                atlas.add_image_future(id, future.map(move |r| r.map(|s| source_with_size(s, size))))
            }
        }
    }
}

fn is_svg_uri(uri: Option<&str>) -> bool {
    let Some(uri) = uri else { return false };
    if uri.starts_with("data:") {
        return uri.starts_with("data:image/svg+xml");
    }
    let path = uri.split(['?', '#']).next().unwrap_or("");
    let file = path.rsplit('/').next().unwrap_or("");
    file.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("svg"))
}

/// Constructor options; `image` and `image_sub_region` are given to
/// [`BillboardCollection::add`](super::billboard_collection::BillboardCollection::add)
/// separately.
#[derive(Clone, Debug, PartialEq)]
pub struct BillboardOptions {
    pub show: bool,
    pub position: Cartesian3,
    pub pixel_offset: Cartesian2,
    pub eye_offset: Cartesian3,
    pub horizontal_origin: HorizontalOrigin,
    pub vertical_origin: VerticalOrigin,
    pub height_reference: HeightReference,
    pub scale: f64,
    pub color: Color,
    pub rotation: f64,
    pub aligned_axis: Cartesian3,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub scale_by_distance: Option<NearFarScalar>,
    pub translucency_by_distance: Option<NearFarScalar>,
    pub pixel_offset_scale_by_distance: Option<NearFarScalar>,
    pub size_in_meters: bool,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: Option<f64>,
    pub id: Option<String>,
    pub image_id: Option<String>,
    pub image_sub_region: Option<BoundingRectangle>,
    pub outline_color: Color,
    pub outline_width: f64,
    pub split_direction: SplitDirection,
}

impl Default for BillboardOptions {
    fn default() -> Self {
        Self {
            show: true,
            position: Cartesian3::ZERO,
            pixel_offset: Cartesian2::ZERO,
            eye_offset: Cartesian3::ZERO,
            horizontal_origin: HorizontalOrigin::Center,
            vertical_origin: VerticalOrigin::Center,
            height_reference: HeightReference::None,
            scale: 1.0,
            color: Color::WHITE,
            rotation: 0.0,
            aligned_axis: Cartesian3::ZERO,
            width: None,
            height: None,
            scale_by_distance: None,
            translucency_by_distance: None,
            pixel_offset_scale_by_distance: None,
            size_in_meters: false,
            distance_display_condition: None,
            disable_depth_test_distance: None,
            id: None,
            image_id: None,
            image_sub_region: None,
            outline_color: Color::BLACK,
            outline_width: 0.0,
            split_direction: SplitDirection::None,
        }
    }
}

impl BillboardOptions {
    /// The constructor's debug-time range checks.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        let check = |ok: bool, message: &str| if ok { Ok(()) } else { Err(RuntimeError::new(message)) };
        check(
            self.disable_depth_test_distance.is_none_or(|d| d >= 0.0),
            "disableDepthTestDistance must be greater than or equal to 0.0.",
        )?;
        check(
            self.translucency_by_distance.is_none_or(|v| v.far > v.near),
            "translucencyByDistance.far must be greater than translucencyByDistance.near.",
        )?;
        check(
            self.pixel_offset_scale_by_distance.is_none_or(|v| v.far > v.near),
            "pixelOffsetScaleByDistance.far must be greater than pixelOffsetScaleByDistance.near.",
        )?;
        check(
            self.scale_by_distance.is_none_or(|v| v.far > v.near),
            "scaleByDistance.far must be greater than scaleByDistance.near.",
        )?;
        check(
            self.distance_display_condition.is_none_or(|v| v.far > v.near),
            "distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.",
        )
    }
}

/// The collection side of `makeDirty`.
pub(crate) struct UpdateQueue<G: Gl + 'static> {
    pub(crate) to_update: Vec<Billboard<G>>,
    pub(crate) properties_changed: [u32; NUMBER_OF_PROPERTIES],
}

impl<G: Gl + 'static> Default for UpdateQueue<G> {
    fn default() -> Self {
        Self { to_update: Vec::new(), properties_changed: [0; NUMBER_OF_PROPERTIES] }
    }
}

/// The state a collection shares with its billboards.
pub(crate) struct CollectionShared<G: Gl + 'static> {
    pub(crate) id: u64,
    pub(crate) queue: RefCell<UpdateQueue<G>>,
    pub(crate) textures: Rc<TextureHost<G>>,
}

impl<G: Gl + 'static> CollectionShared<G> {
    /// `_updateBillboard`: queue a billboard that is not already dirty and
    /// count the property.
    pub(crate) fn update_billboard(&self, billboard: &Billboard<G>, property: usize) {
        let mut queue = self.queue.borrow_mut();
        if !billboard.0.borrow().dirty {
            queue.to_update.push(billboard.clone());
        }
        queue.properties_changed[property] += 1;
    }
}

pub(crate) struct BillboardState<G: Gl + 'static> {
    pub(crate) o: BillboardOptions,
    pub(crate) actual_position: Cartesian3,
    pub(crate) clamped_position: Option<Cartesian3>,
    pub(crate) translate: Cartesian2,
    pub(crate) cluster_show: bool,
    pub(crate) pick_id: Option<PickId>,
    pub(crate) pick_primitive: Option<Rc<dyn std::any::Any>>,
    pub(crate) dirty: bool,
    pub(crate) index: usize,
    pub(crate) texture: Rc<RefCell<BillboardTexture<G>>>,
    pub(crate) image_id: Option<String>,
    pub(crate) image_size: (Option<f64>, Option<f64>),
    pub(crate) label_dimensions: Option<Cartesian2>,
    pub(crate) label_horizontal_origin: Option<HorizontalOrigin>,
    pub(crate) label_translate: Option<Cartesian2>,
    collection: Option<Weak<CollectionShared<G>>>,
    collection_id: Option<u64>,
}

/// A billboard in a collection. Clones share the billboard.
pub struct Billboard<G: Gl + 'static>(pub(crate) Rc<RefCell<BillboardState<G>>>);

impl<G: Gl + 'static> Clone for Billboard<G> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<G: Gl + 'static> std::fmt::Debug for Billboard<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.0.borrow();
        f.debug_struct("Billboard").field("index", &s.index).field("options", &s.o).finish_non_exhaustive()
    }
}

/// The object a billboard registers with `Context.createPickId`: Cesium's
/// `{ primitive, collection, id }`.
pub struct BillboardPick<G: Gl + 'static> {
    pub primitive: Billboard<G>,
    pub collection: Option<u64>,
    pub id: Option<String>,
}

macro_rules! setter {
    ($(#[$m:meta])* $get:ident, $set:ident, $field:ident: $ty:ty, $index:expr) => {
        pub fn $get(&self) -> $ty {
            self.0.borrow().o.$field.clone()
        }
        $(#[$m])*
        pub fn $set(&self, value: $ty) {
            if self.0.borrow().o.$field != value {
                self.0.borrow_mut().o.$field = value;
                self.make_dirty($index);
            }
        }
    };
}

impl<G: Gl + 'static> Billboard<G> {
    pub(crate) fn new(
        options: BillboardOptions,
        image: Option<BillboardImage<G>>,
        collection: &Rc<CollectionShared<G>>,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        options.validate()?;
        // `_updateClamping` without a scene.
        if options.height_reference != HeightReference::None {
            return Err(RuntimeError::new("Height reference is not supported without a scene."));
        }
        let texture = Rc::new(RefCell::new(BillboardTexture::new(Rc::downgrade(&collection.textures))));
        let sub_region = options.image_sub_region;
        let billboard = Self(Rc::new(RefCell::new(BillboardState {
            actual_position: options.position,
            image_id: options.image_id.clone(),
            o: options,
            clamped_position: None,
            translate: Cartesian2::ZERO,
            cluster_show: true,
            pick_id: None,
            pick_primitive: None,
            dirty: false,
            index,
            texture,
            image_size: (None, None),
            label_dimensions: None,
            label_horizontal_origin: None,
            label_translate: None,
            collection: Some(Rc::downgrade(collection)),
            collection_id: Some(collection.id),
        })));
        if let Some(image) = image {
            let id = billboard.0.borrow().image_id.clone();
            billboard.compute_image_texture_properties(id, Some(&image));
            billboard.load(image);
        }
        if let Some(sub_region) = sub_region {
            let id = billboard.0.borrow().image_id.clone().unwrap_or_default();
            let texture = billboard.0.borrow().texture.clone();
            BillboardTexture::add_image_sub_region(&texture, id, sub_region)?;
        }
        Ok(billboard)
    }

    fn load(&self, image: BillboardImage<G>) {
        let (texture, id, size) = {
            let s = self.0.borrow();
            (s.texture.clone(), s.image_id.clone().unwrap_or_default(), s.image_size)
        };
        BillboardTexture::load_image(&texture, id, image, size);
    }

    /// Whether both handles are the same billboard.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    fn collection(&self) -> Option<Rc<CollectionShared<G>>> {
        self.0.borrow().collection.as_ref().and_then(Weak::upgrade)
    }

    pub(crate) fn collection_id(&self) -> Option<u64> {
        self.0.borrow().collection_id
    }

    /// `makeDirty`.
    pub(crate) fn make_dirty(&self, property: usize) {
        if let Some(collection) = self.collection() {
            collection.update_billboard(self, property);
            self.0.borrow_mut().dirty = true;
        }
    }

    /// A snapshot of the constructor options as they now stand.
    pub fn options(&self) -> BillboardOptions {
        self.0.borrow().o.clone()
    }

    setter!(show, set_show, show: bool, SHOW_INDEX);
    setter!(pixel_offset, set_pixel_offset, pixel_offset: Cartesian2, PIXEL_OFFSET_INDEX);
    setter!(eye_offset, set_eye_offset, eye_offset: Cartesian3, EYE_OFFSET_INDEX);
    setter!(horizontal_origin, set_horizontal_origin, horizontal_origin: HorizontalOrigin, HORIZONTAL_ORIGIN_INDEX);
    setter!(vertical_origin, set_vertical_origin, vertical_origin: VerticalOrigin, VERTICAL_ORIGIN_INDEX);
    setter!(scale, set_scale, scale: f64, SCALE_INDEX);
    setter!(color, set_color, color: Color, COLOR_INDEX);
    setter!(rotation, set_rotation, rotation: f64, ROTATION_INDEX);
    setter!(aligned_axis, set_aligned_axis, aligned_axis: Cartesian3, ALIGNED_AXIS_INDEX);
    setter!(
        /// Marks the color attribute dirty, where the flag is packed.
        size_in_meters, set_size_in_meters, size_in_meters: bool, COLOR_INDEX
    );
    setter!(outline_color, set_outline_color, outline_color: Color, SDF_INDEX);
    setter!(outline_width, set_outline_width, outline_width: f64, SDF_INDEX);
    setter!(split_direction, set_split_direction, split_direction: SplitDirection, SPLIT_DIRECTION_INDEX);

    pub fn position(&self) -> Cartesian3 {
        self.0.borrow().o.position
    }
    /// Sets the position and the actual position.
    pub fn set_position(&self, value: Cartesian3) {
        if self.position() != value {
            {
                let mut s = self.0.borrow_mut();
                s.o.position = value;
                s.actual_position = value;
            }
            self.make_dirty(POSITION_INDEX);
        }
    }

    pub fn height_reference(&self) -> HeightReference {
        self.0.borrow().o.height_reference
    }
    /// Without a scene, only `NONE` is accepted.
    pub fn set_height_reference(&self, value: HeightReference) -> Result<(), RuntimeError> {
        if value != self.height_reference() {
            if value != HeightReference::None {
                return Err(RuntimeError::new("Height reference is not supported without a scene."));
            }
            self.0.borrow_mut().o.height_reference = value;
            self.make_dirty(POSITION_INDEX);
        }
        Ok(())
    }

    fn check_range(value: Option<(f64, f64)>, message: &str) -> Result<(), RuntimeError> {
        if value.is_some_and(|(near, far)| far <= near) {
            return Err(RuntimeError::new(message));
        }
        Ok(())
    }

    pub fn scale_by_distance(&self) -> Option<NearFarScalar> {
        self.0.borrow().o.scale_by_distance
    }
    pub fn set_scale_by_distance(&self, value: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        Self::check_range(value.map(|v| (v.near, v.far)), "far distance must be greater than near distance.")?;
        if self.scale_by_distance() != value {
            self.0.borrow_mut().o.scale_by_distance = value;
            self.make_dirty(SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn translucency_by_distance(&self) -> Option<NearFarScalar> {
        self.0.borrow().o.translucency_by_distance
    }
    pub fn set_translucency_by_distance(&self, value: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        Self::check_range(value.map(|v| (v.near, v.far)), "far distance must be greater than near distance.")?;
        if self.translucency_by_distance() != value {
            self.0.borrow_mut().o.translucency_by_distance = value;
            self.make_dirty(TRANSLUCENCY_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn pixel_offset_scale_by_distance(&self) -> Option<NearFarScalar> {
        self.0.borrow().o.pixel_offset_scale_by_distance
    }
    pub fn set_pixel_offset_scale_by_distance(&self, value: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        Self::check_range(value.map(|v| (v.near, v.far)), "far distance must be greater than near distance.")?;
        if self.pixel_offset_scale_by_distance() != value {
            self.0.borrow_mut().o.pixel_offset_scale_by_distance = value;
            self.make_dirty(PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.0.borrow().o.distance_display_condition
    }
    /// As in Cesium, the range check runs only when the value changes.
    pub fn set_distance_display_condition(&self, value: Option<DistanceDisplayCondition>) -> Result<(), RuntimeError> {
        if self.distance_display_condition() != value {
            Self::check_range(value.map(|v| (v.near, v.far)), "far distance must be greater than near distance.")?;
            self.0.borrow_mut().o.distance_display_condition = value;
            self.make_dirty(DISTANCE_DISPLAY_CONDITION);
        }
        Ok(())
    }

    pub fn disable_depth_test_distance(&self) -> Option<f64> {
        self.0.borrow().o.disable_depth_test_distance
    }
    pub fn set_disable_depth_test_distance(&self, value: Option<f64>) -> Result<(), RuntimeError> {
        if value.is_some_and(|v| v < 0.0) {
            return Err(RuntimeError::new("disableDepthTestDistance must be greater than or equal to 0.0."));
        }
        if self.disable_depth_test_distance() != value {
            self.0.borrow_mut().o.disable_depth_test_distance = value;
            self.make_dirty(DISABLE_DEPTH_DISTANCE);
        }
        Ok(())
    }

    /// The width in pixels: the set width, else the image's.
    pub fn width(&self) -> Option<f64> {
        let s = self.0.borrow();
        s.o.width.or_else(|| s.texture.borrow().width())
    }
    pub fn set_width(&self, value: Option<f64>) {
        if self.0.borrow().o.width != value {
            self.0.borrow_mut().o.width = value;
            self.make_dirty(IMAGE_INDEX_INDEX);
        }
    }
    /// The height in pixels: the set height, else the image's.
    pub fn height(&self) -> Option<f64> {
        let s = self.0.borrow();
        s.o.height.or_else(|| s.texture.borrow().height())
    }
    pub fn set_height(&self, value: Option<f64>) {
        if self.0.borrow().o.height != value {
            self.0.borrow_mut().o.height = value;
            self.make_dirty(IMAGE_INDEX_INDEX);
        }
    }

    pub fn id(&self) -> Option<String> {
        self.0.borrow().o.id.clone()
    }
    /// Sets the id, also on an allocated pick object.
    pub fn set_id(&self, value: Option<String>) -> Result<(), RuntimeError> {
        self.0.borrow_mut().o.id = value.clone();
        let s = self.0.borrow();
        if let Some(pick_id) = &s.pick_id {
            if let Some(pick) = pick_id.object_as::<ScenePickObject>() {
                let mut replaced = ScenePickObject::new(pick.primitive.clone(), pick.primitive_show.clone());
                replaced.id = value.map(MetadataValue::String);
                pick_id.set_object(Rc::new(replaced))?;
            }
        }
        Ok(())
    }

    /// The image id, `None` without an image.
    pub fn image(&self) -> Option<String> {
        self.0.borrow().texture.borrow().id().map(String::from)
    }
    /// The `image` setter: `None` unloads; an image without an id gets its
    /// URI or a new GUID as its id.
    pub fn set_image(&self, image: Option<BillboardImage<G>>) {
        match image {
            None => {
                let texture = self.0.borrow().texture.clone();
                texture.borrow_mut().unload();
            }
            Some(image) => {
                self.compute_image_texture_properties(None, Some(&image));
                self.load(image);
            }
        }
    }
    /// `setImage(id, image)`.
    pub fn set_image_with_id(&self, id: impl Into<String>, image: BillboardImage<G>) {
        self.compute_image_texture_properties(Some(id.into()), Some(&image));
        self.load(image);
    }
    /// `setImageSubRegion(id, subRegion)`: shows part of an atlas image.
    pub fn set_image_sub_region(
        &self,
        id: impl Into<String>,
        sub_region: BoundingRectangle,
    ) -> Result<(), RuntimeError> {
        let texture = self.0.borrow().texture.clone();
        BillboardTexture::add_image_sub_region(&texture, id.into(), sub_region)
    }
    /// `setImageTexture`: share another billboard's image state.
    pub fn set_image_texture(&self, source: &Billboard<G>) {
        let (s, t) = (source.0.borrow().texture.clone(), self.0.borrow().texture.clone());
        BillboardTexture::clone_into(&s, &t);
    }

    /// `_computeImageTextureProperties`.
    fn compute_image_texture_properties(&self, id: Option<String>, image: Option<&BillboardImage<G>>) {
        let mut s = self.0.borrow_mut();
        s.image_size = (None, None);
        let Some(image) = image else {
            s.image_id = Some(create_guid());
            return;
        };
        let uri = image.uri();
        s.image_id = Some(id.or_else(|| uri.map(String::from)).unwrap_or_else(create_guid));
        let has_size_in_pixels = s.o.width.is_some() && s.o.height.is_some() && !s.o.size_in_meters;
        if has_size_in_pixels && is_svg_uri(uri) {
            s.image_size = (s.o.width.map(|w| w.min(SVG_MAX_SIZE_PX)), s.o.height.map(|h| h.min(SVG_MAX_SIZE_PX)));
        }
    }

    /// Whether the image is in the atlas.
    pub fn ready(&self) -> bool {
        self.0.borrow().texture.borrow().ready()
    }
    pub fn load_state(&self) -> BillboardLoadState {
        self.0.borrow().texture.borrow().load_state()
    }
    pub fn load_error(&self) -> Option<String> {
        self.0.borrow().texture.borrow().load_error().map(String::from)
    }
    /// The image's atlas index, or -1.
    pub fn image_index(&self) -> i64 {
        self.0.borrow().texture.borrow().index()
    }
    /// Whether the image changed since the billboard was last written.
    pub fn texture_dirty(&self) -> bool {
        self.0.borrow().texture.borrow().dirty
    }
    pub(crate) fn set_texture_dirty(&self, value: bool) {
        self.0.borrow().texture.borrow_mut().dirty = value;
    }
    /// The image's texture coordinates in the atlas.
    pub fn compute_texture_coordinates(&self) -> Result<BoundingRectangle, RuntimeError> {
        self.0.borrow().texture.borrow().compute_texture_coordinates()
    }

    pub fn cluster_show(&self) -> bool {
        self.0.borrow().cluster_show
    }
    pub fn set_cluster_show(&self, value: bool) {
        if self.cluster_show() != value {
            self.0.borrow_mut().cluster_show = value;
            self.make_dirty(SHOW_INDEX);
        }
    }

    /// `_translate`, the glyph translation labels use.
    pub fn translate(&self) -> Cartesian2 {
        self.0.borrow().translate
    }
    /// `_setTranslate`.
    pub fn set_translate(&self, value: Cartesian2) {
        if self.translate() != value {
            self.0.borrow_mut().translate = value;
            self.make_dirty(PIXEL_OFFSET_INDEX);
        }
    }

    /// The position used for rendering: the clamped position, else the
    /// position projected for the scene mode.
    pub fn actual_position(&self) -> Cartesian3 {
        let s = self.0.borrow();
        s.clamped_position.unwrap_or(s.actual_position)
    }
    /// `_setActualPosition`.
    pub(crate) fn set_actual_position(&self, value: Cartesian3) {
        {
            let mut s = self.0.borrow_mut();
            if s.clamped_position.is_none() {
                s.actual_position = value;
            }
        }
        self.make_dirty(POSITION_INDEX);
    }

    pub fn index(&self) -> usize {
        self.0.borrow().index
    }
    pub fn is_dirty(&self) -> bool {
        self.0.borrow().dirty
    }
    pub fn is_attached(&self) -> bool {
        self.0.borrow().collection.is_some()
    }

    /// The pick color, once allocated.
    pub fn pick_id(&self) -> Option<Color> {
        self.0.borrow().pick_id.as_ref().map(|p| p.color)
    }
    /// `getPickId`: allocates the pick id on first use.
    pub fn get_pick_id<H: Gl>(&self, context: &Context<H>) -> Result<Color, RuntimeError> {
        if let Some(color) = self.pick_id() {
            return Ok(color);
        }
        let (id, collection, primitive) = {
            let s = self.0.borrow();
            (s.o.id.clone(), s.collection_id, s.pick_primitive.clone())
        };
        let primitive = primitive.unwrap_or_else(|| {
            Rc::new(BillboardPick { primitive: self.clone(), collection, id: id.clone() }) as Rc<dyn std::any::Any>
        });
        let mut object = ScenePickObject::new(primitive, Rc::new(std::cell::Cell::new(true)));
        object.id = id.map(MetadataValue::String);
        let pick_id = context.create_pick_id(Rc::new(object))?;
        let color = pick_id.color;
        self.0.borrow_mut().pick_id = Some(pick_id);
        Ok(color)
    }

    /// `_destroy`: releases the image, the pick id and the collection.
    pub(crate) fn destroy(&self) {
        let texture = self.0.borrow().texture.clone();
        texture.borrow_mut().unload();
        let mut s = self.0.borrow_mut();
        if let Some(pick_id) = s.pick_id.take() {
            pick_id.destroy();
        }
        s.collection = None;
        s.collection_id = None;
    }

    /// `Billboard._computeActualPosition` for an unclamped billboard.
    pub fn compute_actual_position(
        position: Cartesian3,
        state: &SceneTransformState<'_>,
        model_matrix: &Matrix4,
    ) -> Option<Cartesian3> {
        if state.mode == SceneMode::Scene3D {
            return Some(position);
        }
        SceneTransforms::compute_actual_ellipsoid_position(state, model_matrix.multiply_by_point(position))
    }

    /// `computeScreenSpacePosition` with the collection's model matrix:
    /// window coordinates of the position moved by the eye offset, plus the
    /// pixel offset and translation.
    pub fn compute_screen_space_position(
        &self,
        model_matrix: &Matrix4,
        state: &SceneTransformState<'_>,
        camera: &mut Camera,
    ) -> Result<Option<Cartesian2>, RuntimeError> {
        if !self.is_attached() {
            return Err(RuntimeError::new("Billboard must be in a collection.  Was it removed?"));
        }
        let (position, eye_offset, offset) = {
            let s = self.0.borrow();
            (s.clamped_position.unwrap_or(s.o.position), s.o.eye_offset, s.o.pixel_offset + s.translate)
        };
        let world = model_matrix.multiply_by_point(position);
        Ok(SceneTransforms::world_with_eye_offset_to_window_coordinates(state, camera, world, eye_offset)?
            .map(|w| w + offset))
    }

    /// `Billboard.getScreenSpaceBoundingBox`.
    pub fn screen_space_bounding_box(&self, screen_space_position: Cartesian2) -> BoundingRectangle {
        let scale = self.scale();
        let width = self.width().unwrap_or(f64::NAN) * scale;
        let height = self.height().unwrap_or(f64::NAN) * scale;
        let mut x = screen_space_position.x;
        match self.horizontal_origin() {
            HorizontalOrigin::Right => x -= width,
            HorizontalOrigin::Center => x -= width * 0.5,
            HorizontalOrigin::Left => {}
        }
        let mut y = screen_space_position.y;
        match self.vertical_origin() {
            VerticalOrigin::Bottom | VerticalOrigin::Baseline => y -= height,
            VerticalOrigin::Center => y -= height * 0.5,
            VerticalOrigin::Top => {}
        }
        BoundingRectangle::new(x, y, width, height)
    }

    /// Cesium's `equals`.
    pub fn equals(&self, other: &Self) -> bool {
        if self.ptr_eq(other) {
            return true;
        }
        let (a, b) = (self.0.borrow(), other.0.borrow());
        a.o.id == b.o.id
            && a.o.position == b.o.position
            && a.texture.borrow().id() == b.texture.borrow().id()
            && a.o.show == b.o.show
            && a.o.scale == b.o.scale
            && a.o.vertical_origin == b.o.vertical_origin
            && a.o.horizontal_origin == b.o.horizontal_origin
            && a.o.height_reference == b.o.height_reference
            && a.o.color == b.o.color
            && a.o.pixel_offset == b.o.pixel_offset
            && a.translate == b.translate
            && a.o.eye_offset == b.o.eye_offset
            && a.o.scale_by_distance == b.o.scale_by_distance
            && a.o.translucency_by_distance == b.o.translucency_by_distance
            && a.o.pixel_offset_scale_by_distance == b.o.pixel_offset_scale_by_distance
            && a.o.distance_display_condition == b.o.distance_display_condition
            && a.o.disable_depth_test_distance == b.o.disable_depth_test_distance
            && a.o.split_direction == b.o.split_direction
    }
}
