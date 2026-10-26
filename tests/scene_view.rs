mod common;
use cesium_rs::{core::*, renderer::*, scene::view::camera_equal, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone)]
struct Host(bool);
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([800, 600])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        Ok(match (name, args.first()) {
            ("getParameter", _) => GlValue::Number(0.),
            ("getExtension", Some(GlValue::String(name))) if self.0 && name == "WEBGL_depth_texture" => {
                GlValue::Object(1)
            }
            _ => GlValue::Null,
        })
    }
}
#[test]
fn view_owns_and_destroys_all_its_resources() {
    run(
        "scene",
        "ViewResources",
        |_| EXACT,
        |_, a| {
            let context = Context::new_webgl1(Host(a[0] == true));
            let resources = Rc::new(RefCell::new(ViewResources::new(&context, a[1] == true).unwrap()));
            let mut view = SceneView::default();
            view.set_resources(resources.clone()).unwrap();
            assert!(Rc::ptr_eq(&view.resources::<Host>().unwrap(), &resources));
            let before = {
                let r = resources.borrow();
                json!({"globe":r.globe_depth.is_some(),"oit":r.oit.is_some(),"classificationSupported":r.translucent_tile_classification.is_supported()})
            };
            view.destroy().unwrap();
            view.destroy().unwrap();
            assert!(view.resources::<Host>().is_none());
            let r = resources.borrow();
            let mut destroyed = vec![
                r.pick_framebuffer.as_ref().unwrap().is_destroyed(),
                r.pick_depth_framebuffer.is_destroyed(),
                r.scene_framebuffer.is_destroyed(),
                r.edge_framebuffer.is_destroyed(),
            ];
            if let Some(globe) = &r.globe_depth {
                destroyed.push(globe.is_destroyed());
            }
            destroyed.push(r.globe_translucency_framebuffer.is_destroyed());
            if let Some(oit) = &r.oit {
                destroyed.push(oit.is_destroyed());
            }
            destroyed.push(r.translucent_tile_classification.is_destroyed());
            let mut result = before;
            result["destroyed"] = json!(destroyed);
            Some(Out::Ok(result))
        },
    );
}
fn reference_camera() -> Camera {
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let mut frustum = PerspectiveFrustum::new(std::f64::consts::FRAC_PI_3, 4. / 3.);
    frustum.near = 1.;
    frustum.far = 1e9;
    camera.frustum = CameraFrustum::Perspective(frustum);
    camera.position = Cartesian3::new(0., 0., 2e7);
    camera.direction = Cartesian3::new(0., 0., -1.);
    camera.up = Cartesian3::new(0., 1., 0.);
    camera.right = Cartesian3::new(1., 0., 0.);
    camera
}

#[test]
fn view_fitting_and_binning_match_pinned_source() {
    run(
        "scene",
        "View",
        |_| TIGHT,
        |f, a| match f {
            "frustums" => {
                let o = &a[0];
                let mode = SceneMode::from_value(o["mode"].as_i64().unwrap() as i32).unwrap();
                let options = SceneOptions {
                    far_to_near_ratio: 1000.,
                    logarithmic_depth_far_to_near_ratio: 1e9,
                    near_to_far_distance_2d: n(&a[3]),
                    ..Default::default()
                };
                let z = n(&o["position"][2]);
                let mut view = SceneView::default();
                view.update_frustums([1., 1e9], z, o["log"].as_bool().unwrap(), mode, n(&a[1]), n(&a[2]), &options)
                    .unwrap();
                Some(Out::Ok(json!({
                    "cameraZ": num(z),
                    "frustums": view.frustum_commands_list.iter().map(|f| json!({
                        "near": num(f.near),
                        "far": num(f.far),
                        "indices": f.indices,
                        "counts": f.commands.iter().map(Vec::len).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })))
            }
            "cameraEqual" => {
                let epsilon = n(&a[0]);
                let delta = n(&a[1]);
                let component = s(&a[2]);
                let a_camera = reference_camera();
                let mut b_camera = a_camera.clone_camera();
                match component {
                    "position" => b_camera.position.x += delta,
                    "direction" => b_camera.direction.x += delta,
                    "up" => b_camera.up.x += delta,
                    "right" => b_camera.right.x += delta,
                    "frustum" => {
                        if let CameraFrustum::Perspective(frustum) = &mut b_camera.frustum {
                            frustum.near += delta;
                        }
                    }
                    _ => panic!("unknown component {component}"),
                }
                Some(Out::Ok(json!(camera_equal(&a_camera, &b_camera, epsilon))))
            }
            _ => None,
        },
    );
}

#[test]
fn frustum_bin_insertion_matches_pinned_source() {
    run(
        "scene",
        "ViewBins",
        |_| EXACT,
        |_, a| {
            let debug = a[0].as_bool().unwrap();
            let count = a[1].as_u64().unwrap() as usize;
            let pass = match u(&a[2]) {
                2 => Pass::Globe,
                8 => Pass::Opaque,
                9 => Pass::Translucent,
                other => panic!("unexpected pass {other}"),
            };
            let values = a[3].as_array().unwrap();
            let (near, far, closest) = (n(&values[0]), n(&values[1]), values[2].as_bool().unwrap());
            let mut view = SceneView::default();
            view.frustum_commands_list =
                (0..count).map(|i| FrustumCommands::new(i as f64 + 1., i as f64 + 1. + count as f64 * 2.)).collect();
            let mut command = SceneCommand::new("bin", pass, Rc::new(|_| Ok(())));
            command.metadata.execute_in_closest_frustum = closest;
            let overlap = view.insert_into_bin(command.clone(), near, far, debug);
            let statistics = if debug {
                let mut bins = serde_json::Map::new();
                bins.insert(overlap.to_string(), json!(1));
                json!({"totalCommands": 1, "commandsInFrustums": bins})
            } else {
                Value::Null
            };
            Some(Out::Ok(json!({
                "indices": view.frustum_commands_list.iter().map(|f| f.indices).collect::<Vec<_>>(),
                "counts": view.frustum_commands_list.iter().map(|f| f.commands.iter().map(Vec::len).collect::<Vec<_>>()).collect::<Vec<_>>(),
                "overlap": if debug { json!(overlap) } else { Value::Null },
                "statistics": statistics,
            })))
        },
    );
}

#[test]
fn frustum_commands_reference() {
    run(
        "scene",
        "FrustumCommands",
        |_| EXACT,
        |f, a| {
            let mut commands;
            if f == "construct" {
                let near = if a[0].is_null() { 0. } else { n(&a[0]) };
                let far = if a[1].is_null() { 0. } else { n(&a[1]) };
                commands = FrustumCommands::new(near, far);
            } else {
                commands = FrustumCommands::new(2., 8.);
                commands.indices[Pass::Opaque as usize] = 2;
                commands.indices[Pass::Translucent as usize] = 1;
                for pass in [Pass::Opaque, Pass::Opaque, Pass::Translucent] {
                    commands.commands[pass as usize].push(SceneCommand::new("test", pass, Rc::new(|_| Ok(()))));
                }
            }
            let mut result = json!({
                "near": num(commands.near),
                "far": num(commands.far),
                "passes": commands.commands.len(),
                "indices": commands.indices,
                "lengths": commands.commands.iter().map(Vec::len).collect::<Vec<_>>(),
            });
            if f == "construct" {
                result["numberOfPasses"] = json!(Pass::NUMBER_OF_PASSES);
            } else {
                result["opaqueIndex"] = json!(commands.indices[Pass::Opaque as usize]);
                result["translucentIndex"] = json!(commands.indices[Pass::Translucent as usize]);
            }
            Some(Out::Ok(result))
        },
    );
}

#[test]
fn packed_depth_decoding_matches_reference() {
    run(
        "scene",
        "PickDepth",
        |_| TIGHT,
        |_, a| {
            Some(Out::Ok(num(PickDepth::<Host>::decode_depth(
                a[0].as_array().unwrap().iter().map(|p| u(p) as u8).collect::<Vec<_>>().try_into().unwrap(),
            ))))
        },
    );
}
fn source(v: &Value) -> ShaderSource {
    ShaderSource {
        sources: v["sources"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().into()).collect(),
        defines: v
            .get("defines")
            .and_then(Value::as_array)
            .map_or_else(Vec::new, |a| a.iter().map(|s| s.as_str().unwrap().into()).collect()),
        include_built_ins: v["includeBuiltIns"].as_bool().unwrap_or(true),
        ..Default::default()
    }
}
#[test]
fn oit_shader_and_render_state_variants_match_reference() {
    run(
        "scene",
        "OIT",
        |_| EXACT,
        |_, a| {
            let base = ShaderProgramOptions {
                vertex_shader_source: ShaderSource::new("void main(){gl_Position=vec4(1.0);}"),
                fragment_shader_source: source(&a[0]),
                attribute_locations: Default::default(),
            };
            let kinds =
                if a[1] == true { vec![OitDrawKind::Mrt] } else { vec![OitDrawKind::Color, OitDrawKind::Alpha] };
            let result=kinds.into_iter().map(|kind|{
                let shader=kind.shader(&base).fragment_shader_source;
                let state=kind.render_state(&RenderState{depth_mask:a[2]==true,..Default::default()});
                let mut blending=json!(state.blending);
                let c=state.blending.color;
                blending["color"]=json!([c.red,c.green,c.blue,c.alpha]);
                json!({"source":{"sources":shader.sources,"defines":shader.defines,"includeBuiltIns":shader.include_built_ins},"state":{"depthMask":state.depth_mask,"blending":blending}})
            }).collect::<Vec<_>>();
            Some(Out::Ok(json!(result)))
        },
    );
}
