//! Native lifecycle tests for `VoxelPrimitive`, `TimeDynamicPointCloud`,
//! `PointCloudEyeDomeLighting` and the 3D Tiles voxel provider contract.
use cesium_rs::core::*;
use cesium_rs::renderer::*;
use cesium_rs::scene::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([16, 16])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        let parameter = match args.first() {
            Some(GlValue::Number(value)) => Some(*value),
            _ => None,
        };
        Ok(if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            GlValue::Object(self.next.get())
        } else {
            match name {
                "getParameter" if matches!(parameter, Some(value) if value == 0x846D as f64 || value == 0x846E as f64 || value == 0xD3A as f64) => {
                    GlValue::Numbers(vec![1., 4096.])
                }
                "getParameter" => GlValue::Number(4096.),
                "getExtension" => GlValue::Object(0),
                "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
                "checkFramebufferStatus" => GlValue::Number(36053.),
                _ => GlValue::Null,
            }
        })
    }
}

fn frame_state(context: &Context<Host>) -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(16., 16.));
    let uniform = camera.uniform_camera();
    let culling = CullingVolume::from_bounding_sphere(&BoundingSphere::new(Cartesian3::new(3., 0., 0.), 100.));
    let mut frame = FrameState::new(
        UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)),
        culling,
        [16, 16],
    );
    frame.context = Some(FrameResource::new(context.clone()));
    frame.passes.render = true;
    frame
}

#[test]
fn voxel_primitive_default_provider_lifecycle_and_properties() {
    let mut primitive = VoxelPrimitive::<Host>::new(VoxelPrimitiveOptions::default()).unwrap();
    assert!(!primitive.ready());
    assert_eq!(primitive.shape(), VoxelShapeType::Box);
    assert_eq!(primitive.min_bounds(), Cartesian3::new(-1., -1., -1.));
    assert_eq!(primitive.max_bounds(), Cartesian3::new(1., 1., 1.));
    assert!(primitive.bounding_sphere().is_some());

    primitive.set_level_blend_factor(2.0);
    assert_eq!(primitive.level_blend_factor(), 1.0);
    primitive.set_level_blend_factor(-1.0);
    assert_eq!(primitive.level_blend_factor(), 0.0);
    primitive.set_screen_space_error(8.0);
    primitive.set_step_size(0.5);
    primitive.set_show(false);
    assert!(!primitive.show());
    primitive.set_show(true);
    primitive.set_min_bounds(Cartesian3::new(-2., -2., -2.));
    assert_eq!(primitive.min_bounds(), Cartesian3::new(-2., -2., -2.));

    let context = Context::new_webgl2(Host::default());
    let mut frame = frame_state(&context);
    primitive.update(&mut frame).unwrap();
    assert!(primitive.ready());
    assert!(primitive.traversal().is_some());
    let mut second = frame_state(&context);
    second.frame_number = 2;
    primitive.update(&mut second).unwrap();
    assert!(primitive.statistics().textures_byte_length > 0);
    primitive.destroy().unwrap();
    assert_eq!(primitive.statistics().textures_byte_length, 0);
}

#[test]
fn ndc_aabb_of_a_unit_box_is_the_full_screen() {
    let mut shape = VoxelBoxShape::default();
    assert!(shape.update(
        Matrix4::IDENTITY,
        Cartesian3::new(-1., -1., -1.),
        Cartesian3::new(1., 1., 1.),
        None,
        None,
    ));
    let ndc = oriented_bounding_box_to_ndc_aabb(&shape.oriented_bounding_box, &Matrix4::IDENTITY);
    assert_eq!(ndc, Cartesian4::new(-1., -1., 1., 1.));
}

#[test]
fn point_cloud_eye_dome_lighting_support_and_destruction() {
    let context = Context::new_webgl2(Host::default());
    assert!(PointCloudEyeDomeLighting::<Host>::is_supported(&context));
    let mut lighting = PointCloudEyeDomeLighting::<Host>::new();
    assert_eq!(lighting.strength(), 1.0);
    assert_eq!(lighting.radius(), 1.0);
    lighting.destroy().unwrap();
    assert!(lighting.is_destroyed());
}

#[test]
fn time_dynamic_point_cloud_interval_scheduling_and_memory() {
    let start = JulianDate::new(2451545., 0., TimeStandard::Tai);
    let intervals: TimeIntervalCollection<TimeDynamicPointCloudInterval> =
        TimeIntervalCollection::from_julian_date_array(
            &[start, start.add_seconds(10.), start.add_seconds(20.)],
            cesium_rs::core::time_interval_collection::DateArrayOptions::default(),
            |_interval, index| Some(TimeDynamicPointCloudInterval {
                uri: format!("frame-{index}.pnts"),
                transform: None,
            }),
            |a, b| a == b,
        );
    let clock = Clock::new_at(
        cesium_rs::core::clock::ClockOptions {
            start_time: Some(start),
            current_time: Some(start.add_seconds(5.)),
            clock_range: Some(ClockRange::Clamped),
            multiplier: Some(10.),
            should_animate: Some(true),
            can_animate: Some(true),
            ..Default::default()
        },
        cesium_rs::core::clock::get_timestamp(),
    )
    .unwrap();
    let mut cloud = TimeDynamicPointCloud::<Host>::new(TimeDynamicPointCloudOptions {
        clock,
        intervals,
        show: true,
        model_matrix: Matrix4::IDENTITY,
        shadows: ShadowMode::Enabled,
        maximum_memory_usage: 256.,
        shading: PointCloudShadingOptions::default(),
        clipping_planes: None,
    });
    assert_eq!(cloud.total_memory_usage_in_bytes(), 0);
    assert!((cloud.average_load_time() - 0.05).abs() < f64::EPSILON);
    cloud.make_style_dirty();
    let context = Context::new_webgl2(Host::default());
    let mut frame = frame_state(&context);
    cloud.update(&mut frame).unwrap();
    assert!(cloud.bounding_sphere().is_none());
    cloud.destroy().unwrap();
}

#[test]
fn tiles_voxel_provider_validates_and_maps_shapes() {
    let missing = serde_json::json!({});
    assert!(cesium_rs::scene::cesium_3d_tiles_voxel_provider::validate(&missing)
        .unwrap_err()
        .to_string()
        .contains("Root must have content"));
    let region = serde_json::json!({
        "boundingVolume": {"region": [-1.0, -0.5, 1.0, 0.5, 10.0, 20.0]}
    });
    let shape = cesium_rs::scene::cesium_3d_tiles_voxel_provider::get_shape(&region).unwrap();
    assert_eq!(shape.shape, VoxelShapeType::Ellipsoid);
    assert_eq!(shape.min_bounds, Some(Cartesian3::new(-1.0, -0.5, 10.0)));
    assert_eq!(shape.max_bounds, Some(Cartesian3::new(1.0, 0.5, 20.0)));
    assert_eq!(shape.shape_transform, Matrix4::from_scale(Ellipsoid::WGS84.radii));

    let box_tile = serde_json::json!({
        "boundingVolume": {"box": [0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 8.0]}
    });
    let shape = cesium_rs::scene::cesium_3d_tiles_voxel_provider::get_shape(&box_tile).unwrap();
    assert_eq!(shape.shape, VoxelShapeType::Box);
    assert_eq!(shape.min_bounds, Some(Cartesian3::new(-2.0, -4.0, -8.0)));
    assert_eq!(shape.max_bounds, Some(Cartesian3::new(2.0, 4.0, 8.0)));

    let cylinder = serde_json::json!({
        "boundingVolume": {"extensions": {"3DTILES_bounding_volume_cylinder": {
            "minRadius": 0.0, "maxRadius": 10.0, "height": 20.0,
            "translation": [1.0, 2.0, 3.0]
        }}}
    });
    let shape = cesium_rs::scene::cesium_3d_tiles_voxel_provider::get_shape(&cylinder).unwrap();
    assert_eq!(shape.shape, VoxelShapeType::Cylinder);
    assert_eq!(shape.min_bounds, Some(Cartesian3::new(0.0, -std::f64::consts::PI, -7.0)));
    assert_eq!(shape.max_bounds, Some(Cartesian3::new(10.0, std::f64::consts::PI, 13.0)));
}
