//! Scene snapshots consumed by UniformState, without a dependency on Scene.
use super::{context::*, *};
use crate::core::{simon1994_planetary_positions as planets, *};
use std::{collections::HashMap, rc::Rc};

#[derive(Clone, Copy, Debug)]
pub enum UniformFrustum {
    Perspective(PerspectiveFrustum),
    PerspectiveOffCenter(PerspectiveOffCenterFrustum),
    Orthographic(OrthographicFrustum),
    OrthographicOffCenter(OrthographicOffCenterFrustum),
}
impl UniformFrustum {
    pub fn parameters(&self) -> (Matrix4, Option<Matrix4>, [f64; 2], [f64; 4]) {
        let (projection, infinite, near, far, planes) = match self {
            Self::Perspective(f) => {
                let o = f.off_center_frustum();
                (
                    f.projection_matrix(),
                    Some(f.infinite_projection_matrix()),
                    f.near,
                    f.far,
                    [o.top, o.bottom, o.left, o.right],
                )
            }
            Self::PerspectiveOffCenter(f) => (
                f.projection_matrix(),
                Some(f.infinite_projection_matrix()),
                f.near,
                f.far,
                [f.top, f.bottom, f.left, f.right],
            ),
            Self::Orthographic(f) => {
                let o = f.off_center_frustum();
                (f.projection_matrix(), None, f.near, f.far, [o.top, o.bottom, o.left, o.right])
            }
            Self::OrthographicOffCenter(f) => {
                (f.projection_matrix(), None, f.near, f.far, [f.top, f.bottom, f.left, f.right])
            }
        };
        (projection, infinite, [near, far], planes)
    }
    pub fn fov(&self) -> Option<f64> {
        if let Self::Perspective(f) = self {
            Some(f.fov)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct UniformCamera {
    pub view: Matrix4,
    pub inverse_view: Matrix4,
    pub position: Cartesian3,
    pub direction: Cartesian3,
    pub right: Cartesian3,
    pub up: Cartesian3,
    pub cartographic: Option<Cartographic>,
    pub frustum: UniformFrustum,
}
#[derive(Clone, Copy, Debug)]
pub struct UniformLight {
    /// None denotes SunLight. Other directions point from the light.
    pub direction: Option<Cartesian3>,
    pub color: Color,
    pub intensity: f64,
}
impl Default for UniformLight {
    fn default() -> Self {
        Self { direction: None, color: Color::WHITE, intensity: 2. }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct UniformAtmosphere {
    pub hsb_shift: Cartesian3,
    pub light_intensity: f64,
    pub rayleigh_coefficient: Cartesian3,
    pub rayleigh_scale_height: f64,
    pub mie_coefficient: Cartesian3,
    pub mie_scale_height: f64,
    pub mie_anisotropy: f64,
    pub dynamic_lighting: f64,
}
/// Values supplied by Scene each frame. Core performs the coordinate and
/// astronomical calculations; `celestial_rotation` permits a loaded ICRF
/// orientation (or a custom central body) in place of the TEME fallback.
#[derive(Clone)]
pub struct UniformFrame {
    pub camera: UniformCamera,
    pub map_projection: Rc<dyn MapProjection>,
    pub mode: u32,
    pub time: JulianDate,
    pub celestial_rotation: Option<Matrix3>,
    pub pixel_ratio: f64,
    pub light: UniformLight,
    pub frame_number: f64,
    pub morph_time: f64,
    pub fog_density: f64,
    pub fog_visual_density_scalar: f64,
    pub fog_minimum_brightness: f64,
    pub atmosphere: Option<UniformAtmosphere>,
    pub spherical_harmonic_coefficients: Vec<Cartesian3>,
    pub specular_environment_maps_maximum_lod: Option<f64>,
    pub invert_classification_color: Color,
    pub split_position: f64,
    pub maximum_screen_space_error: f64,
    pub background_color: Color,
    pub minimum_disable_depth_test_distance: f64,
    pub gamma: Option<f64>,
}
impl std::fmt::Debug for UniformFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UniformFrame")
            .field("camera", &self.camera)
            .field("mode", &self.mode)
            .field("time", &self.time)
            .finish_non_exhaustive()
    }
}
impl UniformFrame {
    pub fn new(camera: UniformCamera, time: JulianDate) -> Self {
        Self {
            camera,
            time,
            map_projection: Rc::new(GeographicProjection::default()),
            mode: 3,
            celestial_rotation: None,
            pixel_ratio: 1.,
            light: Default::default(),
            frame_number: 0.,
            morph_time: 1.,
            fog_density: 0.,
            fog_visual_density_scalar: 1.,
            fog_minimum_brightness: 0.03,
            atmosphere: None,
            spherical_harmonic_coefficients: vec![],
            specular_environment_maps_maximum_lod: None,
            invert_classification_color: Color::WHITE,
            split_position: 0.5,
            maximum_screen_space_error: 2.,
            background_color: Color::BLACK,
            minimum_disable_depth_test_distance: 0.,
            gamma: None,
        }
    }
    pub fn use_earth_orientation(&mut self, eop: &EarthOrientationParameters, xys: &Iau2006XysData) {
        self.celestial_rotation = Some(Transforms::compute_icrf_to_central_body_fixed_matrix(&self.time, eop, xys));
    }
}

/// Optional automatic texture inputs; the environment map defaults to the
/// context's white cube. Other absent samplers produce a clear missing-input
/// error if a shader actually uses them.
pub struct FrameTextures<G: Gl> {
    pub textures: HashMap<String, TextureBinding<G>>,
}
impl<G: Gl> Default for FrameTextures<G> {
    fn default() -> Self {
        Self { textures: HashMap::new() }
    }
}
impl<G: Gl> Context<G> {
    pub fn update_frame(&self, frame: &UniformFrame, textures: &FrameTextures<G>) -> Result<()> {
        let cache = self.cache()?;
        let mut bindings = textures.textures.clone();
        if !bindings.contains_key("czm_environmentMap") {
            bindings.insert("czm_environmentMap".into(), TextureBinding::CubeMap(self.default_cube_map()?));
        }
        for (name, texture) in &bindings {
            let uniform =
                automatic_uniforms().get(name).ok_or_else(|| RuntimeError::new("Unknown automatic texture"))?;
            let expected = match uniform.datatype {
                35678 => webgl_constants::TEXTURE_2D,
                35680 => webgl_constants::TEXTURE_CUBE_MAP,
                _ => return Err(RuntimeError::new("Automatic uniform is not a sampler")),
            };
            self.same(texture.sampled().context())?;
            ensure(
                texture.sampled().target() == expected && !texture.sampled().is_destroyed(),
                "Invalid automatic texture",
            )?;
        }
        let [width, height] = self.drawing_buffer_size()?;
        let mut state = self.uniform_state.borrow_mut();
        state.viewport = Viewport { x: 0, y: 0, width, height };
        state.update(frame, width)?;
        *cache.automatic_textures.borrow_mut() = bindings;
        Ok(())
    }
}
impl UniformState {
    pub fn update_camera_snapshot(&mut self, camera: &UniformCamera, ellipsoid: &Ellipsoid) -> Result<()> {
        self.update_camera(camera.view, camera.inverse_view, camera.position);
        if let Some(frame) = &mut self.frame {
            Rc::make_mut(frame).camera = camera.clone();
        }
        self.view_3d = self.camera_view_3d();
        let (projection, infinite, frustum, planes) = camera.frustum.parameters();
        self.update_frustum(projection, infinite, frustum[0], frustum[1]);
        self.entire_frustum = frustum;
        self.frustum_planes = planes;
        self.orthographic_in_3d = self.scene_mode != 2 && matches!(camera.frustum, UniformFrustum::Orthographic(_));
        let (height, normal, surface) = if let Some(c) = camera.cartographic {
            (
                c.height,
                ellipsoid.geodetic_surface_normal_cartographic(&c),
                Some(ellipsoid.cartographic_to_cartesian(&Cartographic::new(c.longitude, c.latitude, 0.))),
            )
        } else {
            (
                -ellipsoid.maximum_radius,
                if camera.position.magnitude_squared() > 0. { camera.position.normalize() } else { Cartesian3::ZERO },
                ellipsoid.scale_to_geodetic_surface(camera.position),
            )
        };
        self.set_scalar("czm_eyeHeight", height);
        if let Some(surface) = surface {
            self.set_vector("czm_eyeEllipsoidNormalEC", camera.view.get_matrix3().multiply_by_vector(normal));
            let enu = self
                .model
                .inverse()?
                .multiply_transformation(&Transforms::east_north_up_to_fixed_frame(surface, ellipsoid));
            self.set_matrix("czm_enuToModel", enu);
            self.set_matrix("czm_modelToEnu", enu.inverse_transformation());
            if (ellipsoid.radii.x - ellipsoid.radii.y).abs()
                <= 1e-15 * ellipsoid.radii.x.abs().max(ellipsoid.radii.y.abs())
            {
                let c = ellipsoid.get_local_curvature(surface);
                self.set_floats("czm_eyeEllipsoidCurvature", &[c.x, c.y]);
            }
        }
        Ok(())
    }
    pub fn update_frustum_snapshot(&mut self, frustum: &UniformFrustum) {
        let (projection, infinite, range, planes) = frustum.parameters();
        self.update_frustum(projection, infinite, range[0], range[1]);
        self.frustum_planes = planes;
    }
    pub fn update(&mut self, frame: &UniformFrame, drawing_buffer_width: u32) -> Result<()> {
        self.frame = Some(Rc::new(frame.clone()));
        self.scene_mode = frame.mode;
        let ellipsoid = frame.map_projection.ellipsoid();
        let camera = &frame.camera;
        self.update_camera_snapshot(camera, ellipsoid)?;
        let width = if frame.mode == 2 { self.frustum_planes[3] - self.frustum_planes[2] } else { 0. };
        self.set_floats("czm_eyeHeight2D", &[width * 0.5, width * width * 0.25]);
        self.frustum_2d_width = width;
        self.view_3d = self.camera_view_3d();
        let rotation =
            frame.celestial_rotation.unwrap_or_else(|| Transforms::compute_teme_to_pseudo_fixed_matrix(&frame.time));
        let sun = rotation.multiply_by_vector(planets::compute_sun_position_in_earth_inertial_frame(&frame.time));
        let sun_direction = sun.normalize();
        let view_rotation = self.view_3d.get_matrix3();
        let sun_ec = view_rotation.multiply_by_vector(sun).normalize();
        self.set_vector("czm_sunPositionWC", sun);
        self.set_vector("czm_sunDirectionWC", sun_direction);
        self.set_vector("czm_sunDirectionEC", sun_ec);
        let moon = rotation.multiply_by_vector(planets::compute_moon_position_in_earth_inertial_frame(&frame.time));
        self.set_vector("czm_moonDirectionEC", view_rotation.multiply_by_vector(moon).normalize());
        let sun_c =
            ellipsoid.cartesian_to_cartographic(sun).ok_or_else(|| RuntimeError::new("Invalid sun position"))?;
        self.set_vector("czm_sunPositionColumbusView", frame.map_projection.project(&sun_c));
        let light = frame.light;
        let (wc, ec) = if let Some(d) = light.direction {
            let wc = d.negate().normalize();
            (wc, view_rotation.multiply_by_vector(wc))
        } else {
            (sun_direction, sun_ec)
        };
        self.set_vector("czm_lightDirectionWC", wc);
        self.set_vector("czm_lightDirectionEC", ec);
        let hdr =
            Cartesian3::new(light.color.red, light.color.green, light.color.blue).multiply_by_scalar(light.intensity);
        let maximum = hdr.x.max(hdr.y).max(hdr.z);
        self.set_vector("czm_lightColorHdr", hdr);
        self.set_vector("czm_lightColor", if maximum > 1. { hdr.divide_by_scalar(maximum) } else { hdr });
        for (name, value) in [
            ("czm_frameNumber", frame.frame_number),
            ("czm_morphTime", frame.morph_time),
            ("czm_pixelRatio", frame.pixel_ratio),
            ("czm_fogDensity", frame.fog_density),
            ("czm_fogVisualDensityScalar", frame.fog_visual_density_scalar),
            ("czm_fogMinimumBrightness", frame.fog_minimum_brightness),
            ("czm_splitPosition", frame.split_position * drawing_buffer_width as f64),
        ] {
            self.set_scalar(name, value);
        }
        self.set_vector("czm_ellipsoidRadii", ellipsoid.radii);
        self.set_vector("czm_ellipsoidInverseRadii", ellipsoid.one_over_radii);
        self.set_color("czm_backgroundColor", frame.background_color);
        self.set_color("czm_invertClassificationColor", frame.invert_classification_color);
        let teme = Transforms::compute_teme_to_pseudo_fixed_matrix(&frame.time);
        self.set(
            "czm_temeToPseudoFixed",
            UniformValue::Matrix { dimension: 3, values: teme.0.iter().map(|v| *v as f32).collect() },
        );
        self.set(
            "czm_sphericalHarmonicCoefficients",
            UniformValue::FloatArray {
                components: 3,
                values: frame
                    .spherical_harmonic_coefficients
                    .iter()
                    .flat_map(|v| [v.x as f32, v.y as f32, v.z as f32])
                    .collect(),
            },
        );
        for (name, value) in [
            ("czm_specularEnvironmentMapsMaximumLOD", frame.specular_environment_maps_maximum_lod),
            ("czm_gamma", frame.gamma),
        ] {
            if let Some(value) = value {
                self.set_scalar(name, value);
            } else {
                self.remove(name);
            }
        }
        if let Some(a) = frame.atmosphere {
            self.set_vector("czm_atmosphereHsbShift", a.hsb_shift);
            self.set_vector("czm_atmosphereRayleighCoefficient", a.rayleigh_coefficient);
            self.set_vector("czm_atmosphereMieCoefficient", a.mie_coefficient);
            for (name, value) in [
                ("czm_atmosphereLightIntensity", a.light_intensity),
                ("czm_atmosphereRayleighScaleHeight", a.rayleigh_scale_height),
                ("czm_atmosphereMieScaleHeight", a.mie_scale_height),
                ("czm_atmosphereMieAnisotropy", a.mie_anisotropy),
                ("czm_atmosphereDynamicLighting", a.dynamic_lighting),
            ] {
                self.set_scalar(name, value);
            }
        }
        // This value is derived at lookup so per-draw viewport changes apply too.
        self.remove("czm_geometricToleranceOverMeter");
        let distance = frame.minimum_disable_depth_test_distance.powi(2);
        self.set_scalar("czm_minimumDisableDepthTestDistance", if distance == f64::INFINITY { -1. } else { distance });
        Ok(())
    }
    pub fn frame_state(&self) -> Option<&UniformFrame> {
        self.frame.as_deref()
    }
    fn camera_view_3d(&self) -> Matrix4 {
        let Some(frame) = self.frame.as_deref() else {
            return self.view_3d;
        };
        let camera = &frame.camera;
        let ellipsoid = frame.map_projection.ellipsoid();
        if self.scene_mode == 3 {
            camera.view
        } else {
            let swizzle = |p: Cartesian3| Cartesian3::new(p.y, p.z, p.x);
            let mut p = swizzle(camera.position);
            if self.scene_mode == 2 {
                p.z = self.frustum_2d_width * 0.5;
            }
            let mut c = frame.map_projection.unproject(p);
            c.longitude = c.longitude.clamp(-std::f64::consts::PI, std::f64::consts::PI);
            c.latitude = c.latitude.clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
            let p = ellipsoid.cartographic_to_cartesian(&c);
            let enu = Transforms::east_north_up_to_fixed_frame(p, ellipsoid);
            Matrix4::compute_view(
                p,
                enu.multiply_by_point_as_vector(swizzle(camera.direction)),
                enu.multiply_by_point_as_vector(swizzle(camera.up)),
                enu.multiply_by_point_as_vector(swizzle(camera.right)),
            )
        }
    }
    fn set_floats(&mut self, name: &str, values: &[f64]) {
        self.set(name, UniformValue::Float(values.iter().map(|v| *v as f32).collect()));
    }
    fn set_scalar(&mut self, name: &str, value: f64) {
        self.set_floats(name, &[value]);
    }
    fn set_vector(&mut self, name: &str, v: Cartesian3) {
        self.set_floats(name, &[v.x, v.y, v.z]);
    }
    fn set_color(&mut self, name: &str, c: Color) {
        self.set_floats(name, &[c.red, c.green, c.blue, c.alpha]);
    }
    fn set_matrix(&mut self, name: &str, m: Matrix4) {
        self.set(name, UniformValue::Matrix { dimension: 4, values: m.0.iter().map(|v| *v as f32).collect() });
    }
}
