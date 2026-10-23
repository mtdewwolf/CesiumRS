#![recursion_limit = "256"]
mod common;
use cesium_rs::{core::*, renderer::*, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{collections::VecDeque, rc::Rc};

fn frame() -> FrameState {
    let mut c = Camera::new(CameraOptions::new(800., 600.));
    let camera = c.uniform_camera();
    let culling = c.frustum.compute_culling_volume(camera.position, camera.direction, camera.up);
    FrameState::new(UniformFrame::new(camera, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, [800, 600])
}
#[test]
fn frame_state_defaults() {
    run(
        "scene",
        "FrameState",
        |_| EXACT,
        |_, a| {
            let mut f = frame();
            let services = &a[0];
            f.context = services.get(0).cloned().map(FrameResource::new);
            f.credit_display = services.get(1).cloned().map(FrameResource::new);
            f.job_scheduler = services.get(2).cloned().map(FrameResource::new);
            let service = |r: &Option<FrameResource>| r.as_ref().and_then(|r| r.get::<Value>()).cloned();
            let shadow = &f.shadow_state;
            Some(Out::Ok(
                json!({"context":service(&f.context),"creditDisplay":service(&f.credit_display),"jobScheduler":service(&f.job_scheduler),
        "commandList":[],"panoramaCommandList":[],"shadowMaps":[],"brdfLutGenerator":f.brdf_lut_generator.as_ref().map(|_|true),"environmentMap":f.environment_map.as_ref().map(|_|true),
        "sphericalHarmonicCoefficients":f.spherical_harmonic_coefficients.as_ref().map(|v|v.iter().map(Enc::enc).collect::<Vec<_>>()),"specularEnvironmentMaps":f.specular_environment_maps.as_ref().map(|_|true),"specularEnvironmentMapsMaximumLOD":f.specular_environment_maps_maximum_lod,
        "mode":f.mode.value(),"morphTime":f.morph_time,"frameNumber":f.frame_number,"newFrame":f.new_frame,"cameraUnderground":f.camera_underground,"globeTranslucencyState":f.globe_translucency_state.as_ref().map(|_|true),"occluder":Value::Null,
        "maximumScreenSpaceError":num(f.maximum_screen_space_error),"pixelRatio":f.pixel_ratio,"passes":{"render":f.passes.render,"pick":f.passes.pick,"pickVoxel":f.passes.pick_voxel,"depth":f.passes.depth,"postProcess":f.passes.post_process,"offscreen":f.passes.offscreen},"afterRender":[],"scene3DOnly":f.scene_3d_only,
        "fog":{"enabled":f.fog.enabled,"renderable":f.fog.renderable,"density":num(f.fog.density),"visualDensityScalar":num(f.fog.visual_density_scalar),"sse":num(f.fog.sse),"minimumBrightness":num(f.fog.minimum_brightness)},"atmosphere":f.atmosphere.as_ref().map(|_|true),"verticalExaggeration":f.vertical_exaggeration,"verticalExaggerationRelativeHeight":f.vertical_exaggeration_relative_height,
        "shadowState":{"shadowsEnabled":shadow.shadows_enabled,"shadowMaps":[],"lightShadowMaps":[],"nearPlane":shadow.near_plane,"farPlane":shadow.far_plane,"closestObjectSize":shadow.closest_object_size,"lastDirtyTime":shadow.last_dirty_time,"outOfView":shadow.out_of_view},"splitPosition":f.split_position,"frustumSplits":f.frustum_splits,"backgroundColor":f.background_color.map(|c|c.enc()),"light":f.light.map(|_|true),"minimumDisableDepthTestDistance":f.minimum_disable_depth_test_distance,"invertClassification":f.invert_classification,"invertClassificationColor":f.invert_classification_color.map(|c|c.enc()),"useLogDepth":f.use_log_depth,"tilesetPassState":f.tileset_pass_state.map(|s|s.pass.value()),"minimumTerrainHeight":f.minimum_terrain_height,"pickingMetadata":f.picking_metadata,"pickedMetadataInfo":f.picked_metadata_info,"edgeVisibilityRequested":f.edge_visibility_requested}),
            ))
        },
    );
}

fn source_frustum(o: &Value) -> UniformFrustum {
    let near = o["near"].as_f64().unwrap_or(1.);
    let far = o["far"].as_f64().unwrap_or(1000.);
    if o["mode"] == 2 {
        UniformFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
            -1000., 1000., 1000., -1000., near, far,
        ))
    } else {
        let mut f = PerspectiveFrustum::new(std::f64::consts::PI / 3., 4. / 3.);
        f.near = near;
        f.far = far;
        UniformFrustum::Perspective(f)
    }
}
fn fit(o: &Value, commands: &Value) -> Value {
    let mut f = frame();
    f.mode = SceneMode::from_value(o["mode"].as_i64().unwrap_or(3) as i32).unwrap();
    f.use_log_depth = o["log"].as_bool().unwrap_or(false);
    let position = o.get("position").map(c3).unwrap_or(Cartesian3::new(0., 0., 100.));
    let direction = Cartesian3::new(0., 0., -1.);
    let up = Cartesian3::UNIT_Y;
    let frustum = source_frustum(o);
    f.uniform.camera.position = position;
    f.camera_position_local = position;
    f.uniform.camera.direction = direction;
    f.uniform.camera.up = up;
    f.uniform.camera.frustum = frustum;
    f.culling_volume = match frustum {
        UniformFrustum::Perspective(p) => p.compute_culling_volume(position, direction, up),
        UniformFrustum::OrthographicOffCenter(p) => p.compute_culling_volume(position, direction, up),
        _ => unreachable!(),
    };
    for (id, c) in commands.as_array().unwrap().iter().enumerate() {
        let pass = [
            Pass::Environment,
            Pass::Compute,
            Pass::Globe,
            Pass::TerrainClassification,
            Pass::Cesium3DTileEdges,
            Pass::Cesium3DTile,
            Pass::Cesium3DTileClassification,
            Pass::Cesium3DTileClassificationIgnoreShow,
            Pass::Opaque,
            Pass::Translucent,
            Pass::Voxels,
            Pass::GaussianSplats,
            Pass::Overlay,
        ][c["pass"].as_u64().unwrap_or(8) as usize];
        let mut command = SceneCommand::new(id.to_string(), pass, Rc::new(|_| Ok(())));
        command.metadata.bounding_volume =
            c.get("sphere").map(|s| BoundingSphere::new(Cartesian3::new(n(&s[0]), n(&s[1]), n(&s[2])), n(&s[3])));
        command.metadata.cull = c["cull"].as_bool().unwrap_or(true);
        command.metadata.receive_shadows = c["shadows"].as_bool().unwrap_or(false);
        command.metadata.execute_in_closest_frustum = c["closest"].as_bool().unwrap_or(false);
        f.command_list.push(command);
    }
    let options = SceneOptions {
        far_to_near_ratio: o["ratio"].as_f64().unwrap_or(10.),
        logarithmic_depth_far_to_near_ratio: o["logRatio"].as_f64().unwrap_or(1e9),
        near_to_far_distance_2d: o["step"].as_f64().unwrap_or(100.),
        debug_show_frustums: true,
        ..Default::default()
    };
    let mut view = SceneView::default();
    view.create_potentially_visible_set(&mut f, &options).unwrap();
    let statistics = view.debug_frustum_statistics.unwrap();
    json!({"frustums":view.frustum_commands_list.iter().map(|frustum|json!({"near":num(frustum.near),"far":num(frustum.far),"commands":frustum.commands.iter().map(|cs|cs.iter().map(|c|json!([c.id.parse::<usize>().unwrap(),c.metadata.debug_overlapping_frustums])).collect::<Vec<_>>()).collect::<Vec<_>>()})).collect::<Vec<_>>(),"splits":f.frustum_splits.iter().map(|v|num(*v)).collect::<Vec<_>>(),"compute":view.compute_commands.iter().map(|c|c.id.parse::<usize>().unwrap()).collect::<Vec<_>>(),"overlay":view.overlay_commands.iter().map(|c|c.id.parse::<usize>().unwrap()).collect::<Vec<_>>(),"statistics":{"totalCommands":statistics.total_commands,"commandsInFrustums":statistics.commands_in_frustums.iter().map(|(k,v)|(k.to_string(),json!(v))).collect::<serde_json::Map<_,_>>()},"shadows":{"near":num(f.shadow_state.near_plane),"far":num(f.shadow_state.far_plane),"closest":num(f.shadow_state.closest_object_size)}})
}
#[test]
fn view_reference() {
    run("scene", "SceneView", |_| TIGHT, |_, a| Some(Out::Ok(fit(&a[0], &a[1]))));
}

struct ReadyContent;
impl TilesetContent for ReadyContent {
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics::default()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(true)
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(None)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
fn traverse(a: &[Value]) -> Result<Value, RuntimeError> {
    let mut t = Cesium3DTileset::from_json(
        Resource::new("https://example.com/tileset.json").unwrap(),
        &a[0],
        serde_json::from_value(a[1].clone()).unwrap(),
        None,
    )?;
    let mut ids = Vec::new();
    let mut queue = VecDeque::from([t.root.unwrap()]);
    while let Some(id) = queue.pop_front() {
        ids.push(id);
        queue.extend(&t.tiles[id].children);
    }
    for id in a[2].as_array().unwrap() {
        let id = ids[u(id) as usize];
        if !t.tiles[id].has_empty_content() {
            t.install_content(id, Box::new(ReadyContent))?;
        }
    }
    let config = &a[3];
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    camera.position = c3(&config["position"]);
    camera.direction = config.get("direction").map(c3).unwrap_or(Cartesian3::new(-1., 0., 0.)).normalize();
    camera.up = Cartesian3::UNIT_Z;
    camera.right = camera.direction.cross(camera.up);
    if let CameraFrustum::Perspective(f) = &mut camera.frustum {
        f.fov = config["fov"].as_f64().unwrap_or(std::f64::consts::PI / 3.);
        f.near = 1.;
        f.far = 1e8;
    }
    let snapshot = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
    let mut f = FrameState::new(
        UniformFrame::new(snapshot, JulianDate::new(2451545., 0., TimeStandard::Tai)),
        culling,
        [800, 600],
    );
    f.uniform.pixel_ratio = config["pixelRatio"].as_f64().unwrap_or(1.);
    f.camera_time_since_moved = config["timeSinceMoved"].as_f64().unwrap_or(1.);
    f.frame_number = 5;
    t.pre_passes_update(&mut f)?;
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::from_value(u(&a[4]) as i32).unwrap()))?;
    let index = |id: usize| ids.iter().position(|&i| i == id).unwrap();
    let mut requested = t
        .requested_tiles
        .iter()
        .map(|&id| json!({"id":index(id),"priority":num(t.tiles[id].priority)}))
        .collect::<Vec<_>>();
    requested.sort_by_key(|v| v["id"].as_u64());
    Ok(
        json!({"selected":t.selected_tiles.iter().copied().map(index).collect::<Vec<_>>(),"requested":requested,"empty":t.empty_tiles.iter().copied().map(index).collect::<Vec<_>>(),"visited":t.statistics.visited,"mixed":t.has_mixed_content,"selection":t.selected_tiles.iter().map(|&id|json!([index(id),t.tiles[id].traversal.selection_depth,t.tiles[id].traversal.final_resolution])).collect::<Vec<_>>(),"styled":t.selected_tiles_to_style.iter().copied().map(index).collect::<Vec<_>>()}),
    )
}
#[test]
fn traversal_reference() {
    run(
        "scene",
        "TilesetTraversal",
        |_| Tol { rel: 1e-12, abs: 1e-7, vector: false },
        |_, a| {
            Some(match traverse(a) {
                Ok(v) => Out::Ok(v),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

fn expand(a: &[Value]) -> Result<Value, RuntimeError> {
    let mut t = Cesium3DTileset::from_json(
        Resource::new("https://example.com/tileset.json").unwrap(),
        &a[0],
        TilesetOptions::default(),
        None,
    )?;
    let root = t.root.unwrap();
    let description = t.tiles[root].implicit_tileset.clone().unwrap();
    let coordinates = t.tiles[root].implicit_coordinates.unwrap();
    let resource = t.tiles[root].content_resource.as_ref().unwrap();
    let subtree = ImplicitSubtree::from_subtree_json(
        resource,
        implicit_subtree::SubtreeSource::Json(a[1].clone()),
        &description,
        coordinates,
        &mut |r| {
            let name = r.url().rsplit('/').next().unwrap().to_owned();
            a[2][&name]
                .as_array()
                .map(|bytes| bytes.iter().map(|v| u(v) as u8).collect())
                .ok_or_else(|| MetadataError::Runtime(format!("missing {}", r.url())))
        },
    )
    .map_err(|e| RuntimeError::new(e.to_string()))?;
    t.expand_subtree(root, Rc::new(subtree), description, coordinates)?;
    let mut queue = VecDeque::from([root]);
    let mut tiles = Vec::new();
    while let Some(id) = queue.pop_front() {
        let tile = &t.tiles[id];
        queue.extend(&tile.children);
        let h = &tile.header;
        let contents = h
            .get("contents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| h.get("content").cloned().into_iter().collect());
        tiles.push(json!({"depth":tile.depth,"error":num(tile.geometric_error*tile.computed_transform.get_maximum_scale()),"bounds":h["boundingVolume"],"sphere":tile.bounding_sphere.enc(),"children":tile.children.len(),"placeholder":tile.implicit_tileset.is_some(),"coordinates":tile.implicit_coordinates.map(|c|json!([c.level,c.x,c.y,c.z])),"contents":contents.iter().map(|c|c.get("uri").or_else(||c.get("url")).cloned().unwrap_or(Value::Null)).collect::<Vec<_>>()}));
    }
    Ok(json!({"total":t.statistics.number_of_tiles_total,"tiles":tiles}))
}
#[test]
fn implicit_expansion_reference() {
    run(
        "scene",
        "TilesetExpansion",
        |_| Tol { rel: 1e-12, abs: 1e-7, vector: true },
        |_, a| {
            Some(match expand(a) {
                Ok(v) => Out::Ok(v),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

fn shader_source(v: &Value) -> ShaderSource {
    ShaderSource {
        sources: v["sources"].as_array().unwrap().iter().map(|v| s(v).into()).collect(),
        defines: v["defines"].as_array().map_or_else(Vec::new, |v| v.iter().map(|v| s(v).into()).collect()),
        ..Default::default()
    }
}
fn shader_snapshot(s: &ShaderSource) -> Value {
    json!({"sources":s.sources,"defines":s.defines,"includeBuiltIns":s.include_built_ins,"pickColorQualifier":s.pick_color_qualifier.map(|q|q.as_str())})
}
#[test]
fn derived_command_reference() {
    run(
        "scene",
        "DerivedCommand",
        |_| EXACT,
        |_, a| {
            let mut options = ShaderProgramOptions {
                vertex_shader_source: shader_source(&a[0]["vertex"]),
                fragment_shader_source: shader_source(&a[0]["fragment"]),
                attribute_locations: Default::default(),
            };
            let mut state = RenderState { depth_mask: false, ..Default::default() };
            state.blending.enabled = true;
            for op in a[1].as_array().unwrap() {
                match s(op) {
                    "log" => options = DerivedCommand::logarithmic_depth(&options),
                    "hdr" => options = DerivedCommand::hdr(&options),
                    "depth" => {
                        options = DerivedCommand::depth_only(&options);
                        state = DerivedCommand::depth_only_render_state(&state);
                    }
                    "pick" => {
                        options = DerivedCommand::pick(&options, "u_pickColor");
                        state = DerivedCommand::pick_render_state(&state);
                    }
                    "metadata" => {
                        let m = &a[2];
                        let class = MetadataClassProperty::from_json("p", &m["property"], &Default::default()).unwrap();
                        let transform = m["transform"]
                            .as_array()
                            .map(|v| (MetadataValue::from_json(&v[0]), MetadataValue::from_json(&v[1])));
                        let info = PickMetadataShader::new(Some("s".into()), "c".into(), "p".into(), &class, transform)
                            .unwrap();
                        options = DerivedCommand::pick_metadata(&options, &info);
                        state = DerivedCommand::pick_render_state(&state);
                    }
                    _ => unreachable!(),
                }
            }
            Some(Out::Ok(
                json!({"vertex":shader_snapshot(&options.vertex_shader_source),"fragment":shader_snapshot(&options.fragment_shader_source),"state":{"depthMask":state.depth_mask,"colorMask":state.color_mask,"blend":state.blending.enabled}}),
            ))
        },
    );
}
#[test]
fn camera_events_reference() {
    run(
        "scene",
        "SceneCameraEvents",
        |_| EXACT,
        |_, a| {
            let mut camera = Camera::new(CameraOptions::new(800., 600.));
            camera.position = c3(&a[0]);
            let mut view = SceneView::default();
            view.initialize_camera(&camera);
            let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let start = events.clone();
            let end = events.clone();
            camera.move_start.add_event_listener(move |_| start.lock().unwrap().push("start"));
            camera.move_end.add_event_listener(move |_| end.lock().unwrap().push("end"));
            let mut result = Vec::new();
            for step in a[1].as_array().unwrap() {
                if let Some(position) = step.get("position") {
                    camera.position = c3(position);
                }
                if let Some(fov) = step["fov"].as_f64() {
                    if let CameraFrustum::Perspective(f) = &mut camera.frustum {
                        f.fov = fov;
                    }
                }
                events.lock().unwrap().clear();
                let changed = view.check_for_camera_updates(&mut camera, n(&step["time"]), n(&a[2]));
                result.push(json!({"changed":changed,"events":*events.lock().unwrap()}));
            }
            Some(Out::Ok(json!(result)))
        },
    );
}
#[test]
fn dynamic_error_reference() {
    run(
        "scene",
        "TilesetDynamicError",
        |_| TIGHT,
        |_, a| {
            let mut t = Cesium3DTileset::from_json(
                Resource::new("https://example.com/tileset.json").unwrap(),
                &a[0],
                TilesetOptions::default(),
                None,
            )
            .unwrap();
            t.model_matrix = m4(&a[3]);
            let mut f = frame();
            f.uniform.camera.position = c3(&a[1]);
            f.uniform.camera.direction = c3(&a[2]);
            f.uniform.camera.cartographic = Ellipsoid::WGS84.cartesian_to_cartographic(c3(&a[1]));
            t.pre_passes_update(&mut f).unwrap();
            Some(Out::Ok(num(t.dynamic_screen_space_error_computed_density)))
        },
    );
}
