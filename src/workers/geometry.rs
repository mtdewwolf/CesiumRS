//! Geometry worker entry points, corresponding to CesiumJS `Workers/create*Geometry`.
//!
//! Value-owned Core descriptions replace structured-cloned JavaScript objects:
//! centers, ellipsoids and nested descriptions already retain their Rust types.
//! Ground polylines use Core's bundled approximate terrain heights, so there is
//! no asynchronous asset initialization. Packed inputs use the same layout and
//! offset as the Core packers; slices must contain a complete valid description,
//! just as for the Core unpack methods.

use crate::core::*;

/// Either a typed description or a description packed at an explicit offset.
#[derive(Clone, Copy, Debug)]
pub enum GeometryInput<'a, T> {
    Description(&'a T),
    Packed { values: &'a [f64], offset: usize },
}

/// Common interface implemented by all 31 built-in geometry worker types.
pub trait WorkerGeometry: Sized {
    const MODULE_NAME: &'static str;
    fn unpack_worker(values: &[f64], offset: usize) -> Option<Self>;
    fn create_worker(&self) -> Option<Geometry>;
}

pub fn create<T: WorkerGeometry>(input: GeometryInput<'_, T>) -> Option<Geometry> {
    match input {
        GeometryInput::Description(description) => description.create_worker(),
        GeometryInput::Packed { values, offset } => T::unpack_worker(values, offset)?.create_worker(),
    }
}

macro_rules! geometry_workers {
    ($(($ty:ident, $function:ident, $unpack:expr, $create:expr)),+ $(,)?) => {
        /// An owned geometry description for heterogeneous geometry batches.
        #[derive(Clone, Debug)]
        pub enum GeometryDescription { $($ty($ty)),+ }

        impl GeometryDescription {
            pub fn module_name(&self) -> &'static str {
                match self { $(Self::$ty(_) => <$ty as WorkerGeometry>::MODULE_NAME),+ }
            }
            pub fn create_geometry(&self) -> Option<Geometry> {
                match self { $(Self::$ty(value) => value.create_worker()),+ }
            }
            pub fn unpack(module_name: &str, values: &[f64], offset: usize) -> Option<Self> {
                match module_name {
                    $(<$ty as WorkerGeometry>::MODULE_NAME =>
                        <$ty as WorkerGeometry>::unpack_worker(values, offset).map(Self::$ty)),+,
                    _ => None,
                }
            }
            pub const MODULE_NAMES: &'static [&'static str] = &[$(concat!("create", stringify!($ty))),+];
        }
        $(
            impl WorkerGeometry for $ty {
                const MODULE_NAME: &'static str = concat!("create", stringify!($ty));
                fn unpack_worker(a: &[f64], offset: usize) -> Option<Self> { ($unpack)(a, offset) }
                fn create_worker(&self) -> Option<Geometry> { ($create)(self) }
            }
            #[doc = concat!("CesiumJS `Workers/", concat!("create", stringify!($ty)), "`: direct or packed geometry creation.")]
            pub fn $function(input: GeometryInput<'_, $ty>) -> Option<Geometry> { create(input) }
        )+
    };
}

geometry_workers! {
    (
        BoxGeometry, create_box_geometry,
        |a, offset| Some(BoxGeometry::unpack(a, offset)),
        |value: &BoxGeometry| value.create_geometry()
    ),
    (
        BoxOutlineGeometry, create_box_outline_geometry,
        |a, offset| Some(BoxOutlineGeometry::unpack(a, offset)),
        |value: &BoxOutlineGeometry| value.create_geometry()
    ),
    (
        CircleGeometry, create_circle_geometry,
        CircleGeometry::unpack,
        |value: &CircleGeometry| value.create_geometry()
    ),
    (
        CircleOutlineGeometry, create_circle_outline_geometry,
        CircleOutlineGeometry::unpack,
        |value: &CircleOutlineGeometry| value.create_geometry()
    ),
    (
        CoplanarPolygonGeometry, create_coplanar_polygon_geometry,
        |a, offset| Some(CoplanarPolygonGeometry::unpack(a, offset)),
        |value: &CoplanarPolygonGeometry| value.create_geometry()
    ),
    (
        CoplanarPolygonOutlineGeometry, create_coplanar_polygon_outline_geometry,
        |a, offset| Some(CoplanarPolygonOutlineGeometry::unpack(a, offset)),
        |value: &CoplanarPolygonOutlineGeometry| value.create_geometry()
    ),
    (
        CorridorGeometry, create_corridor_geometry,
        |a, offset| Some(CorridorGeometry::unpack(a, offset)),
        |value: &CorridorGeometry| value.create_geometry()
    ),
    (
        CorridorOutlineGeometry, create_corridor_outline_geometry,
        |a, offset| Some(CorridorOutlineGeometry::unpack(a, offset)),
        |value: &CorridorOutlineGeometry| value.create_geometry()
    ),
    (
        CylinderGeometry, create_cylinder_geometry,
        CylinderGeometry::unpack,
        |value: &CylinderGeometry| value.create_geometry()
    ),
    (
        CylinderOutlineGeometry, create_cylinder_outline_geometry,
        CylinderOutlineGeometry::unpack,
        |value: &CylinderOutlineGeometry| value.create_geometry()
    ),
    (
        EllipseGeometry, create_ellipse_geometry,
        EllipseGeometry::unpack,
        |value: &EllipseGeometry| value.create_geometry()
    ),
    (
        EllipseOutlineGeometry, create_ellipse_outline_geometry,
        EllipseOutlineGeometry::unpack,
        |value: &EllipseOutlineGeometry| value.create_geometry()
    ),
    (
        EllipsoidGeometry, create_ellipsoid_geometry,
        EllipsoidGeometry::unpack,
        |value: &EllipsoidGeometry| value.create_geometry()
    ),
    (
        EllipsoidOutlineGeometry, create_ellipsoid_outline_geometry,
        EllipsoidOutlineGeometry::unpack,
        |value: &EllipsoidOutlineGeometry| value.create_geometry()
    ),
    (
        FrustumGeometry, create_frustum_geometry,
        |a, offset| Some(FrustumGeometry::unpack(a, offset)),
        |value: &FrustumGeometry| value.create_geometry()
    ),
    (
        FrustumOutlineGeometry, create_frustum_outline_geometry,
        |a, offset| Some(FrustumOutlineGeometry::unpack(a, offset)),
        |value: &FrustumOutlineGeometry| value.create_geometry()
    ),
    (
        GroundPolylineGeometry, create_ground_polyline_geometry,
        GroundPolylineGeometry::unpack,
        |value: &GroundPolylineGeometry| value.create_geometry()
    ),
    (
        PlaneGeometry, create_plane_geometry,
        |a, offset| Some(PlaneGeometry::unpack(a, offset)),
        |value: &PlaneGeometry| Some(value.create_geometry())
    ),
    (
        PlaneOutlineGeometry, create_plane_outline_geometry,
        |a, offset| Some(PlaneOutlineGeometry::unpack(a, offset)),
        |value: &PlaneOutlineGeometry| Some(value.create_geometry())
    ),
    (
        PolygonGeometry, create_polygon_geometry,
        |a, offset| Some(PolygonGeometry::unpack(a, offset)),
        |value: &PolygonGeometry| value.create_geometry()
    ),
    (
        PolygonOutlineGeometry, create_polygon_outline_geometry,
        |a, offset| Some(PolygonOutlineGeometry::unpack(a, offset)),
        |value: &PolygonOutlineGeometry| value.create_geometry()
    ),
    (
        PolylineGeometry, create_polyline_geometry,
        |a, offset| Some(PolylineGeometry::unpack(a, offset)),
        |value: &PolylineGeometry| value.create_geometry()
    ),
    (
        PolylineVolumeGeometry, create_polyline_volume_geometry,
        |a, offset| Some(PolylineVolumeGeometry::unpack(a, offset)),
        |value: &PolylineVolumeGeometry| value.create_geometry()
    ),
    (
        PolylineVolumeOutlineGeometry, create_polyline_volume_outline_geometry,
        |a, offset| Some(PolylineVolumeOutlineGeometry::unpack(a, offset)),
        |value: &PolylineVolumeOutlineGeometry| value.create_geometry()
    ),
    (
        RectangleGeometry, create_rectangle_geometry,
        |a, offset| Some(RectangleGeometry::unpack(a, offset)),
        |value: &RectangleGeometry| value.create_geometry()
    ),
    (
        RectangleOutlineGeometry, create_rectangle_outline_geometry,
        |a, offset| Some(RectangleOutlineGeometry::unpack(a, offset)),
        |value: &RectangleOutlineGeometry| value.create_geometry()
    ),
    (
        SimplePolylineGeometry, create_simple_polyline_geometry,
        |a, offset| Some(SimplePolylineGeometry::unpack(a, offset)),
        |value: &SimplePolylineGeometry| value.create_geometry()
    ),
    (
        SphereGeometry, create_sphere_geometry,
        SphereGeometry::unpack,
        |value: &SphereGeometry| value.create_geometry()
    ),
    (
        SphereOutlineGeometry, create_sphere_outline_geometry,
        SphereOutlineGeometry::unpack,
        |value: &SphereOutlineGeometry| value.create_geometry()
    ),
    (
        WallGeometry, create_wall_geometry,
        |a, offset| Some(WallGeometry::unpack(a, offset)),
        |value: &WallGeometry| value.create_geometry()
    ),
    (
        WallOutlineGeometry, create_wall_outline_geometry,
        |a, offset| Some(WallOutlineGeometry::unpack(a, offset)),
        |value: &WallOutlineGeometry| value.create_geometry()
    ),
}
