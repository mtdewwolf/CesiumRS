mod common;
use cesium_rs::{core::*, renderer::UniformState, scene::*};
use common::*;
use serde_json::{json, Value};
use std::rc::Rc;

fn camera(o: &Value) -> Camera {
    let mut options = CameraOptions::new(1600., 900.);
    options.client_width = 800.;
    options.client_height = 600.;
    if o["mercator"] == true {
        options.map_projection = Rc::new(WebMercatorProjection::default());
    }
    let mut c = Camera::new(options);
    let mode = SceneMode::from_value(o["mode"].as_i64().unwrap() as i32).unwrap();
    c.frustum = if mode == SceneMode::Scene2D {
        CameraFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(-1e7, 1e7, 7.5e6, -7.5e6, 1., 1e9))
    } else if o["ortho"] == true {
        let mut f = OrthographicFrustum::new(2e7, 4. / 3.);
        f.near = 1.;
        f.far = 1e9;
        CameraFrustum::Orthographic(f)
    } else {
        let mut f = PerspectiveFrustum::new(std::f64::consts::PI / 3., 4. / 3.);
        f.near = 1.;
        f.far = 1e9;
        CameraFrustum::Perspective(f)
    };
    c.position = c3(&o["position"]);
    c.direction = -Cartesian3::UNIT_Z;
    c.up = Cartesian3::UNIT_Y;
    c.right = Cartesian3::UNIT_X;
    c.update(mode).unwrap();
    c
}

#[test]
fn reference_transforms() {
    run(
        "scene",
        "SceneTransforms",
        |_| TIGHT,
        |f, a| {
            let result = match f {
                "project" => {
                    let o = &a[0];
                    let mut c = camera(o);
                    let projection: Rc<dyn MapProjection> = if o["mercator"] == true {
                        Rc::new(WebMercatorProjection::default())
                    } else {
                        Rc::new(GeographicProjection::default())
                    };
                    let state = SceneTransformState {
                        mode: c.mode(),
                        morph_time: o["morph"].as_f64().unwrap_or(1.),
                        map_projection: projection.as_ref(),
                        client_dimensions: Cartesian2::new(800., 600.),
                        drawing_buffer_dimensions: Cartesian2::new(1600., 900.),
                    };
                    let before = (c.position, c.view_matrix(), c.frustum);
                    let mut outputs = Vec::new();
                    for p in a[1].as_array().unwrap() {
                        let actual = SceneTransforms::compute_actual_ellipsoid_position(&state, c3(p));
                        let window = SceneTransforms::world_with_eye_offset_to_window_coordinates(
                            &state,
                            &mut c,
                            c3(p),
                            c3(&a[2]),
                        )
                        .unwrap();
                        let buffer = window.map(|p| SceneTransforms::transform_window_to_drawing_buffer(&state, p));
                        outputs.push(json!({"actual":actual.map(|p|p.enc()),"window":window.map(|p|p.enc()),"buffer":buffer.map(|p|p.enc())}));
                    }
                    json!({"outputs":outputs,"unchanged":before==(c.position,c.view_matrix(),c.frustum)})
                }
                "unproject" => {
                    let mut c = camera(&a[0]);
                    let [near, far] = [n(&a[2][0]), n(&a[2][1])];
                    let mut f = c.frustum;
                    match &mut f {
                        CameraFrustum::Perspective(f) => {
                            f.near = near;
                            f.far = far;
                        }
                        CameraFrustum::Orthographic(f) => {
                            f.near = near;
                            f.far = far;
                        }
                        CameraFrustum::OrthographicOffCenter(f) => {
                            f.near = near;
                            f.far = far;
                        }
                    }
                    let mut uniforms = UniformState::default();
                    uniforms.update_camera(c.view_matrix(), c.inverse_view_matrix(), c.position_wc());
                    let (p, infinite, _, _) = f.uniform_frustum().parameters();
                    uniforms.update_frustum(p, infinite, near, far);
                    SceneTransforms::drawing_buffer_to_world_coordinates(
                        c.frustum,
                        &uniforms,
                        &BoundingRectangle::new(n(&a[1][0]), n(&a[1][1]), n(&a[1][2]), n(&a[1][3])),
                        c2(&a[3]),
                        n(&a[4]),
                        a[5].as_bool().unwrap(),
                    )
                    .unwrap()
                    .enc()
                }
                "clip" => SceneTransforms::clip_to_gl_window_coordinates(
                    &BoundingRectangle::new(n(&a[0][0]), n(&a[0][1]), n(&a[0][2]), n(&a[0][3])),
                    c4(&a[1]),
                )
                .enc(),
                _ => return None,
            };
            Some(Out::Ok(result))
        },
    );
}

#[test]
fn camera_origin_reports_invalid_normalization() {
    let mut c = camera(&json!({"mode":3,"position":[0,0,2e7]}));
    let projection = GeographicProjection::default();
    let state = SceneTransformState {
        mode: SceneMode::Scene3D,
        morph_time: 1.,
        map_projection: &projection,
        client_dimensions: Cartesian2::new(800., 600.),
        drawing_buffer_dimensions: Cartesian2::new(1600., 900.),
    };
    let position = c.position_wc();
    assert!(SceneTransforms::world_to_window_coordinates(&state, &mut c, position).is_err());
}

#[test]
fn reference_pick_culling_and_rectangles() {
    run(
        "scene",
        "Picking",
        |_| TIGHT,
        |f, a| {
            Some(Out::Ok(match f {
                "culling" => {
                    let mut c = camera(&a[0]);
                    let mode = c.mode();
                    let viewport = BoundingRectangle::new(n(&a[3][0]), n(&a[3][1]), n(&a[3][2]), n(&a[3][3]));
                    let before = (c.position, c.view_matrix(), c.frustum);
                    let volume =
                        Picking::get_pick_culling_volume(&mut c, mode, c2(&a[1]), n(&a[2][0]), n(&a[2][1]), &viewport)
                            .unwrap();
                    assert_eq!(before, (c.position, c.view_matrix(), c.frustum));
                    json!(volume.planes.iter().map(|p| p.enc()).collect::<Vec<_>>())
                }
                "rectangle" => {
                    let r =
                        Picking::compute_drawing_buffer_rectangle(n(&a[0]), c2(&a[1]), opt(&a[2][0]), opt(&a[2][1]));
                    json!([num(r.x), num(r.y), num(r.width), num(r.height)])
                }
                _ => return None,
            }))
        },
    );
}

#[derive(Clone)]
struct PickHost;
impl cesium_rs::renderer::Gl for PickHost {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(
        &self,
        _: &str,
        _: &[cesium_rs::renderer::GlValue<u32>],
    ) -> Result<cesium_rs::renderer::GlValue<u32>, RuntimeError> {
        Ok(cesium_rs::renderer::GlValue::Null)
    }
}
#[test]
fn reference_pick_spiral_and_identity() {
    use cesium_rs::{renderer::Context, scene::pick_framebuffer::pick_objects_from_pixels};
    run(
        "scene",
        "PickFramebuffer",
        |_| EXACT,
        |_, a| {
            let context = Context::new_webgl2(PickHost);
            let objects: Vec<_> = (0..4).map(Rc::new).collect();
            let ids = [0, 0, 1, 2, 3].map(|i| context.create_pick_id(objects[i].clone()).unwrap());
            let pixels = a[2]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|k| (k.as_u64().unwrap() as u32).to_le_bytes())
                .collect::<Vec<_>>();
            let result = pick_objects_from_pixels(&context, &pixels, u(&a[0]), u(&a[1]), u(&a[3]) as usize);
            // Keep IDs alive until after lookup, as actual draw commands must.
            assert_eq!(ids.len(), 5);
            Some(Out::Ok(json!(result.iter().map(|o| *o.clone().downcast::<i32>().unwrap()).collect::<Vec<_>>())))
        },
    );
}
