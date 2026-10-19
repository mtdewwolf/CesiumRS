//! CesiumJS `GeometryFactory`: descriptions capable of constructing geometry.

use super::Geometry;

pub trait GeometryFactory {
    fn create_geometry(&self) -> Option<Geometry>;
}

macro_rules! factories {
    ($($ty:ty),* $(,)?) => { $(
        impl GeometryFactory for $ty {
            fn create_geometry(&self) -> Option<Geometry> { <$ty>::create_geometry(self) }
        }
    )* };
}

factories!(
    super::BoxGeometry,
    super::BoxOutlineGeometry,
    super::CircleGeometry,
    super::CircleOutlineGeometry,
    super::EllipseGeometry,
    super::EllipseOutlineGeometry,
    super::CoplanarPolygonGeometry,
    super::CoplanarPolygonOutlineGeometry,
    super::CorridorGeometry,
    super::CorridorOutlineGeometry,
    super::CylinderGeometry,
    super::CylinderOutlineGeometry,
    super::EllipsoidGeometry,
    super::EllipsoidOutlineGeometry,
    super::FrustumGeometry,
    super::FrustumOutlineGeometry,
    super::PolygonGeometry,
    super::PolygonOutlineGeometry,
    super::PolylineGeometry,
    super::SimplePolylineGeometry,
    super::PolylineVolumeGeometry,
    super::PolylineVolumeOutlineGeometry,
    super::RectangleGeometry,
    super::RectangleOutlineGeometry,
    super::SphereGeometry,
    super::SphereOutlineGeometry,
    super::WallGeometry,
    super::WallOutlineGeometry
);

impl GeometryFactory for super::PlaneGeometry {
    fn create_geometry(&self) -> Option<Geometry> {
        Some(super::PlaneGeometry::create_geometry(self))
    }
}
impl GeometryFactory for super::PlaneOutlineGeometry {
    fn create_geometry(&self) -> Option<Geometry> {
        Some(super::PlaneOutlineGeometry::create_geometry(self))
    }
}

impl<F: Fn() -> Option<Geometry>> GeometryFactory for F {
    fn create_geometry(&self) -> Option<Geometry> {
        self()
    }
}
