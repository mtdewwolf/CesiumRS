//! Terrain-bounded ground polyline volumes, geodesic/rhumb subdivision and 2D attributes.
use super::*;
#[derive(Clone, Debug)]
pub struct GroundPolylineGeometry {
    pub positions: Vec<Cartesian3>,
    pub width: f64,
    pub granularity: f64,
    pub looped: bool,
    pub arc_type: ArcType,
    pub ellipsoid: Ellipsoid,
    pub mercator: bool,
    pub scene_3d_only: bool,
}
#[derive(Clone, Copy)]
struct Vertex {
    c: Cartographic,
    bottom: Cartesian3,
    top: Cartesian3,
    normal: Cartesian3,
}
fn position(e: &Ellipsoid, c: Cartographic, h: f64) -> Cartesian3 {
    e.cartographic_to_cartesian(&Cartographic::new(c.longitude, c.latitude, h))
}
fn direction(a: Cartesian3, b: Cartesian3) -> Cartesian3 {
    a.subtract(b).normalize()
}
fn tangent(a: Cartesian3, b: Cartesian3, up: Cartesian3) -> Cartesian3 {
    up.cross(direction(a, b).cross(up).normalize())
}
fn eq(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e || (a - b).abs() <= e * a.abs().max(b.abs())
}
fn close(a: Cartesian3, b: Cartesian3) -> bool {
    eq(a.x, b.x, 1e-7) && eq(a.y, b.y, 1e-7) && eq(a.z, b.z, 1e-7)
}
fn right(e: &Ellipsoid, a: Cartographic, b: Cartographic) -> Cartesian3 {
    let bottom = position(e, a, 0.);
    let up = direction(position(e, a, 1000.), bottom);
    direction(position(e, b, 0.), bottom).cross(up).normalize()
}
fn miter(previous: Cartesian3, bottom: Cartesian3, top: Cartesian3, next: Cartesian3) -> Cartesian3 {
    let up = direction(top, bottom);
    let p = tangent(previous, bottom, up);
    let n = tangent(next, bottom, up);
    if eq(p.dot(n), -1., 1e-5) {
        return up.cross(p).normalize();
    }
    let result = n.add(p).normalize();
    if n.dot(up.cross(result)) < 0. {
        result.negate()
    } else {
        result
    }
}
fn break_miter(normal: &mut Cartesian3, start: Cartesian3, end: Cartesian3, top: Cartesian3) -> bool {
    let dot = direction(end, start).dot(*normal);
    if dot > (30f64.to_radians()).cos() || dot < (150f64.to_radians()).cos() {
        let angle =
            if dot < (150f64.to_radians()).cos() { std::f64::consts::FRAC_PI_2 } else { -std::f64::consts::FRAC_PI_2 };
        *normal = Matrix3::from_quaternion(&Quaternion::from_axis_angle(direction(top, end), angle))
            .multiply_by_vector(*normal);
        true
    } else {
        false
    }
}
impl GroundPolylineGeometry {
    pub fn new(positions: Vec<Cartesian3>) -> Option<Self> {
        if positions.len() < 2 {
            return None;
        }
        Some(Self {
            positions,
            width: 1.,
            granularity: 9999.,
            looped: false,
            arc_type: ArcType::Geodesic,
            ellipsoid: Ellipsoid::WGS84,
            mercator: false,
            scene_3d_only: false,
        })
    }
    pub fn set_projection_and_ellipsoid(&mut self, mercator: bool, ellipsoid: Ellipsoid) {
        self.mercator = mercator;
        self.ellipsoid = ellipsoid;
    }
    pub fn packed_length(&self) -> usize {
        1 + self.positions.len() * 3 + 8
    }
    pub fn pack(&self, a: &mut [f64], offset: usize) -> Option<()> {
        let a = a.get_mut(offset..offset.checked_add(self.packed_length())?)?;
        a[0] = self.positions.len() as f64;
        let mut p = 1;
        for c in &self.positions {
            a[p..p + 3].copy_from_slice(&[c.x, c.y, c.z]);
            p += 3;
        }
        a[p..p + 8].copy_from_slice(&[
            self.granularity,
            f64::from(self.looped),
            self.arc_type as u8 as f64,
            self.ellipsoid.radii.x,
            self.ellipsoid.radii.y,
            self.ellipsoid.radii.z,
            f64::from(self.mercator),
            f64::from(self.scene_3d_only),
        ]);
        Some(())
    }
    pub fn unpack(a: &[f64], offset: usize) -> Option<Self> {
        let n = *a.get(offset)? as usize;
        let a = a.get(offset..offset.checked_add(1 + n.checked_mul(3)? + 8)?)?;
        let positions = a[1..1 + n * 3].as_chunks::<3>().0.iter().map(|v| Cartesian3::new(v[0], v[1], v[2])).collect();
        let mut g = Self::new(positions)?;
        let p = 1 + n * 3;
        g.granularity = a[p];
        g.looped = a[p + 1] == 1.;
        g.arc_type = match a[p + 2] as i32 {
            1 => ArcType::Geodesic,
            2 => ArcType::Rhumb,
            _ => return None,
        };
        g.ellipsoid = Ellipsoid::new(a[p + 3], a[p + 4], a[p + 5]);
        g.mercator = a[p + 6] == 1.;
        g.scene_3d_only = a[p + 7] == 1.;
        Some(g)
    }
    pub fn create_geometry(&self) -> Option<Geometry> {
        if self.arc_type == ArcType::None || self.granularity < 0. || !self.granularity.is_finite() {
            return None;
        }
        let e = &self.ellipsoid;
        let looped = self.looped && self.positions.len() > 2;
        let mut split = vec![self.positions[0]];
        let pairs = self.positions.len() - 1 + usize::from(looped);
        for i in 0..pairs {
            let a = self.positions[i];
            let b = self.positions[(i + 1) % self.positions.len()];
            let den = b.y - a.y;
            if den.abs() >= 1e-6 {
                let t = -a.y / den;
                if (0. ..=1.).contains(&t) {
                    let mut intersection = a.add(b.subtract(a).multiply_by_scalar(t));
                    if !close(intersection, a) && !close(intersection, b) {
                        if self.arc_type == ArcType::Rhumb {
                            let ca = e.cartesian_to_cartographic(a)?;
                            let cb = e.cartesian_to_cartographic(b)?;
                            let lon = e.cartesian_to_cartographic(intersection)?.longitude;
                            let c = EllipsoidRhumbLine::new(&ca, &cb, e).find_intersection_with_longitude(lon)?;
                            intersection = e.cartographic_to_cartesian(&c);
                        }
                        if !close(intersection, a) && !close(intersection, b) {
                            split.push(intersection);
                        }
                    }
                }
            }
            if i < self.positions.len() - 1 {
                split.push(b);
            }
        }
        let mut cartographics: Vec<Cartographic> = vec![];
        for p in split {
            let mut c = e.cartesian_to_cartographic(p)?;
            c.height = 0.;
            if cartographics
                .last()
                .is_none_or(|v| (c.longitude - v.longitude).abs() > 1e-10 || (c.latitude - v.latitude).abs() > 1e-10)
            {
                cartographics.push(c);
            }
        }
        if cartographics.len() < 2 {
            return None;
        }
        let mut vertices = vec![];
        for i in 0..cartographics.len() {
            let c = cartographics[i];
            let bottom = position(e, c, 0.);
            let top = position(e, c, 1000.);
            let normal = if !looped && i == 0 {
                right(e, c, cartographics[1])
            } else if !looped && i == cartographics.len() - 1 {
                right(e, cartographics[i - 1], c)
            } else {
                miter(
                    position(e, cartographics[(i + cartographics.len() - 1) % cartographics.len()], 0.),
                    bottom,
                    top,
                    position(e, cartographics[(i + 1) % cartographics.len()], 0.),
                )
            };
            vertices.push(Vertex { c, bottom, top, normal });
            if i + 1 < cartographics.len() || looped {
                let end = cartographics[(i + 1) % cartographics.len()];
                if self.granularity > 0. {
                    let geodesic = if self.arc_type == ArcType::Geodesic {
                        Some(EllipsoidGeodesic::new(&c, &end, e)?)
                    } else {
                        None
                    };
                    let rhumb =
                        if self.arc_type == ArcType::Rhumb { Some(EllipsoidRhumbLine::new(&c, &end, e)) } else { None };
                    let distance = geodesic
                        .as_ref()
                        .map_or_else(|| rhumb.as_ref().unwrap().surface_distance, |g| g.surface_distance);
                    if distance >= self.granularity {
                        let segments = (distance / self.granularity).ceil() as usize;
                        if segments > 2_000_000 {
                            return None;
                        }
                        let step = distance / segments as f64;
                        let mut d = step;
                        let normal = right(e, c, end);
                        for _ in 0..segments - 1 {
                            let c = geodesic.as_ref().map_or_else(
                                || rhumb.as_ref().unwrap().interpolate_using_surface_distance(d),
                                |g| g.interpolate_using_surface_distance(d),
                            );
                            vertices.push(Vertex { c, bottom: position(e, c, 0.), top: position(e, c, 1000.), normal });
                            d += step;
                        }
                    }
                }
            }
        }
        if looped {
            vertices.push(vertices[0]);
        }
        let projection: Box<dyn MapProjection> = if self.mercator {
            Box::new(WebMercatorProjection::new(*e))
        } else {
            Box::new(GeographicProjection::new(*e))
        };
        self.attributes(&vertices, looped, projection.as_ref())
    }
    pub fn project_normal(
        projection: &dyn MapProjection,
        c: Cartographic,
        normal: Cartesian3,
        projected: Cartesian3,
    ) -> Option<Cartesian3> {
        let e = projection.ellipsoid();
        let p = e.cartographic_to_cartesian(&c);
        let mut endpoint = e.cartesian_to_cartographic(p.add(normal))?;
        let flip = (c.longitude - endpoint.longitude).abs() > std::f64::consts::FRAC_PI_2;
        if flip {
            endpoint = e.cartesian_to_cartographic(p.subtract(normal))?;
        }
        endpoint.height = 0.;
        let p = projection.project(&endpoint).subtract(projected);
        let result = Cartesian3::new(p.x, p.y, 0.).normalize();
        Some(if flip { result.negate() } else { result })
    }
    fn attributes(&self, vertices: &[Vertex], looped: bool, projection: &dyn MapProjection) -> Option<Geometry> {
        let n = vertices.len() - 1;
        let count = n.checked_mul(8)?;
        if count > u32::MAX as usize {
            return None;
        }
        let compute2d = !self.scene_3d_only;
        let length3 = vertices.windows(2).map(|v| v[0].top.distance(v[1].top)).sum::<f64>();
        let length2 = if compute2d {
            vertices.windows(2).map(|v| projection.project(&v[0].c).distance(projection.project(&v[1].c))).sum::<f64>()
        } else {
            0.
        };
        let mut arrays: [Vec<f64>; 9] = std::array::from_fn(|_| vec![]);
        let mut positions = vec![];
        let mut end_normal = vertices[0].normal;
        if looped
            && break_miter(&mut end_normal, vertices[vertices.len() - 2].bottom, vertices[0].bottom, vertices[0].top)
        {
            end_normal = end_normal.negate();
        }
        let mut broken = false;
        let (mut sofar3, mut sofar2, mut sum_heights) = (0., 0., 0.);
        for pair in vertices.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let mut start_normal = end_normal;
            if broken {
                start_normal = start_normal.negate();
            }
            end_normal = b.normal;
            broken = break_miter(&mut end_normal, a.bottom, b.bottom, b.top);
            let (mut ca, mut cb) = (a.c, b.c);
            let (mut p2a, mut p2b, mut n2a, mut n2b) =
                (Cartesian3::ZERO, Cartesian3::ZERO, Cartesian3::ZERO, Cartesian3::ZERO);
            if compute2d {
                let nudge = if eq(ca.longitude.abs(), std::f64::consts::PI, 1e-11) {
                    ca.longitude = super::math::sign(cb.longitude) * (ca.longitude.abs() - 1e-11);
                    1
                } else if eq(cb.longitude.abs(), std::f64::consts::PI, 1e-11) {
                    cb.longitude = super::math::sign(ca.longitude) * (cb.longitude.abs() - 1e-11);
                    2
                } else {
                    0
                };
                p2a = projection.project(&ca);
                p2b = projection.project(&cb);
                let forward = direction(p2b, p2a);
                if nudge == 0 || forward.y.abs() > 30f64.to_radians().cos() {
                    n2a = Self::project_normal(projection, ca, start_normal, p2a)?;
                    n2b = Self::project_normal(projection, cb, end_normal, p2b)?;
                } else if nudge == 1 {
                    n2b = Self::project_normal(projection, cb, end_normal, p2b)?;
                    n2a = Cartesian3::new(0., super::math::sign(ca.longitude - cb.longitude.abs()), 0.);
                } else {
                    n2a = Self::project_normal(projection, ca, start_normal, p2a)?;
                    n2b = Cartesian3::new(0., super::math::sign(ca.longitude - cb.longitude), 0.);
                }
            }
            let segment3 = a.top.distance(b.top);
            let encoded = EncodedCartesian3::from_cartesian(a.bottom);
            let offset = b.bottom.subtract(a.bottom);
            let forward = offset.normalize();
            let up = direction(a.top, a.bottom);
            let right = forward.cross(up).normalize();
            let start_plane = up.cross(start_normal).normalize();
            let end_up = direction(b.top, b.bottom);
            let end_plane = end_normal.cross(end_up).normalize();
            let norm3x = segment3 / length3;
            let norm3y = sofar3 / length3;
            let segment2 = p2a.distance(p2b);
            let encoded2 = EncodedCartesian3::from_cartesian(p2a);
            let offset2 = p2b.subtract(p2a);
            let right2 = offset2.normalize();
            let norm2x = segment2 / length2;
            let norm2y = sofar2 / length2;
            for j in 0..8 {
                let side = if j < 4 { 1. } else { -1. };
                let tb = if matches!(j, 2 | 3 | 6 | 7) { 1. } else { -1. };
                let pack = |v: Cartesian3, w: f64| [v.x, v.y, v.z, w];
                arrays[0].extend(pack(encoded.high, offset.x));
                arrays[1].extend(pack(encoded.low, offset.y));
                arrays[2].extend(pack(start_plane, offset.z));
                arrays[3].extend(pack(end_plane, norm3x * side));
                let ny = norm3y * tb;
                arrays[4].extend(pack(right, if ny == 0. && tb < 0. { 9. } else { ny }));
                if compute2d {
                    arrays[5].extend([encoded2.high.x, encoded2.high.y, encoded2.low.x, encoded2.low.y]);
                    arrays[6].extend([offset2.x, offset2.y, right2.y, -right2.x]);
                    arrays[7].extend([-n2a.y, n2a.x, n2b.y, -n2b.x]);
                    let ny = norm2y * tb;
                    arrays[8].extend([norm2x * side, if ny == 0. && tb < 0. { 9. } else { ny }]);
                }
            }
            let rect = Rectangle::from_cartographic_array(&[ca, cb]);
            let (min_h, max_h) =
                ApproximateTerrainHeights::bundled().get_minimum_maximum_heights(&rect, projection.ellipsoid())?;
            sum_heights += min_h.abs();
            sum_heights += max_h.abs();
            let adjust = |v: Vertex| {
                let up = direction(v.top, v.bottom);
                (v.bottom.add(up.multiply_by_scalar(min_h)), v.top.add(up.multiply_by_scalar(max_h - 1000.)))
            };
            let (mut ab, mut at) = adjust(a);
            let (mut bb, mut bt) = adjust(b);
            let nudge = right.multiply_by_scalar(1e-5);
            ab = ab.add(nudge);
            bb = bb.add(nudge);
            at = at.add(nudge);
            bt = bt.add(nudge);
            nudge_xz(&mut ab, &mut bb);
            nudge_xz(&mut at, &mut bt);
            for p in [ab, bb, bt, at] {
                positions.extend([p.x, p.y, p.z]);
            }
            let nudge = right.multiply_by_scalar(-2e-5);
            ab = ab.add(nudge);
            bb = bb.add(nudge);
            at = at.add(nudge);
            bt = bt.add(nudge);
            nudge_xz(&mut ab, &mut bb);
            nudge_xz(&mut at, &mut bt);
            for p in [ab, bb, bt, at] {
                positions.extend([p.x, p.y, p.z]);
            }
            sofar3 += segment3;
            sofar2 += segment2;
        }
        const REF: [u32; 36] = [
            0, 2, 1, 0, 3, 2, 0, 7, 3, 0, 4, 7, 0, 5, 4, 0, 1, 5, 5, 7, 4, 5, 6, 7, 5, 2, 6, 5, 1, 2, 3, 6, 2, 3, 7, 6,
        ];
        let ids = (0..n).flat_map(|i| REF.map(|v| v + i as u32 * 8)).collect::<Vec<_>>();
        let bottom = vertices.iter().map(|v| v.bottom).collect::<Vec<_>>();
        let top = vertices.iter().map(|v| v.top).collect::<Vec<_>>();
        let mut sphere = BoundingSphere::from_bounding_spheres(&[
            BoundingSphere::from_points(&bottom),
            BoundingSphere::from_points(&top),
        ]);
        sphere.radius += sum_heights / (n as f64 * 2.);
        let mut attrs =
            GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        let names = [
            "startHiAndForwardOffsetX",
            "startLoAndForwardOffsetY",
            "startNormalAndForwardOffsetZ",
            "endNormalAndTextureCoordinateNormalizationX",
            "rightNormalAndTextureCoordinateNormalizationY",
            "startHiLo2D",
            "offsetAndRight2D",
            "startEndNormals2D",
            "texcoordNormalization2D",
        ];
        for (i, (name, array)) in names.iter().zip(arrays).enumerate() {
            if i < 5 || compute2d {
                attrs.set_other(name, GeometryAttribute::floats(if i == 8 { 2 } else { 4 }, &array));
            }
        }
        let mut geometry =
            Geometry::new(attrs, Some(IndexDatatype::create_typed_array(count, &ids)), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(sphere);
        Some(geometry)
    }
}
fn nudge_xz(start: &mut Cartesian3, end: &mut Cartesian3) {
    if eq(start.y, 0., 1e-2) {
        *start = start.add(direction(*end, *start).multiply_by_scalar(1e-2));
    } else if eq(end.y, 0., 1e-2) {
        *end = end.add(direction(*start, *end).multiply_by_scalar(1e-2));
    }
}
