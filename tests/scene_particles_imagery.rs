//! Particles and emitters, PointCloudShading, ImageryLayerFeatureInfo,
//! KeyframeNode, ImageryConfiguration and DiscardEmptyTileImagePolicy
//! against CesiumJS (`tools/cesium/fixtures/scene-particles-imagery.mjs`).
mod common;
use cesium_rs::core::math::{set_random_number_seed, to_radians};
use cesium_rs::core::{Cartesian2, Cartesian3, Color};
use cesium_rs::renderer::ContextCapabilities;
use cesium_rs::scene::discard_empty_tile_image_policy::EMPTY_IMAGE_URI;
use cesium_rs::scene::model::{ImageryConfiguration, ImageryLayerSettings};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Map, Value};
use std::rc::Rc;

fn v3(c: Cartesian3) -> Value {
    json!([num(c.x), num(c.y), num(c.z)])
}

fn rgba(c: Color) -> Value {
    json!([num(c.red), num(c.green), num(c.blue), num(c.alpha)])
}

fn state(p: &Particle) -> Value {
    json!({
        "mass": num(p.mass), "position": v3(p.position), "velocity": v3(p.velocity), "life": num(p.life),
        "startColor": rgba(p.start_color), "endColor": rgba(p.end_color), "startScale": num(p.start_scale),
        "endScale": num(p.end_scale), "imageSize": [num(p.image_size.x), num(p.image_size.y)],
        "age": num(p.age()), "normalizedAge": num(p.normalized_age()),
    })
}

fn color_of(v: &Value) -> Color {
    let c = nums(v);
    Color::new(c[0], c[1], c[2], c[3])
}

fn options(o: &Value) -> ParticleOptions {
    let f = |k: &str| o.get(k).and_then(Value::as_f64);
    let present = |k: &str| o.get(k).filter(|v| !v.is_null());
    ParticleOptions {
        mass: f("mass"),
        position: present("position").map(c3),
        velocity: present("velocity").map(c3),
        life: f("life"),
        image: None,
        start_color: present("startColor").map(color_of),
        end_color: present("endColor").map(color_of),
        start_scale: f("startScale"),
        end_scale: f("endScale"),
        image_size: present("imageSize").map(|v| Cartesian2::new(n(&v[0]), n(&v[1]))),
    }
}

#[test]
fn particle() {
    run(
        "scene",
        "Particle",
        |_| EXACT,
        |f, a| {
            let mut p = Particle::new(options(&a[0]));
            if f == "new" {
                return ok(state(&p));
            }
            let mut gravity = |q: &mut Particle, dt: f64| q.velocity.z -= 9.8 * dt;
            let mut drag = |q: &mut Particle, dt: f64| {
                q.velocity *= 1.0 - 0.5 * dt;
                q.mass += dt;
            };
            let steps: Vec<Value> = nums(&a[1])
                .into_iter()
                .map(|dt| {
                    let update: Option<ParticleUpdateFunction> = match a[2].as_str() {
                        Some("gravity") => Some(&mut gravity),
                        Some("drag") => Some(&mut drag),
                        _ => None,
                    };
                    json!([p.update(dt, update), state(&p)])
                })
                .collect();
            ok(Value::Array(steps))
        },
    );
}

fn emitter(kind: &str, arg: &Value) -> Box<dyn ParticleEmitter> {
    let number = arg.as_f64();
    match kind {
        "box" => Box::new(BoxEmitter::new((!arg.is_null()).then(|| c3(arg)))),
        "circle" => Box::new(CircleEmitter::new(number)),
        "cone" => Box::new(ConeEmitter::new(number)),
        _ => Box::new(SphereEmitter::new(number)),
    }
}

fn emit_many(e: &dyn ParticleEmitter, seed: &Value, count: &Value) -> Value {
    set_random_number_seed(u(seed));
    let mut p = Particle::default();
    (0..u(count))
        .map(|_| {
            e.emit(&mut p);
            json!([v3(p.position), v3(p.velocity)])
        })
        .collect()
}

/// An emitter's size property and its emitted particles.
fn emitter_port(kind: &'static str) -> impl Fn(&str, &[Value]) -> Option<Out> {
    move |f: &str, a: &[Value]| {
        let (property, emitted) = match (kind, f) {
            ("box", "set") => {
                let mut e = BoxEmitter::default();
                e.set_dimensions(c3(&a[0]));
                (v3(e.dimensions()), emit_many(&e, &a[1], &a[2]))
            }
            ("box", _) => {
                let e = BoxEmitter::new((!a[0].is_null()).then(|| c3(&a[0])));
                (v3(e.dimensions()), emit_many(&e, &a[1], &a[2]))
            }
            ("circle", _) => {
                let mut e = CircleEmitter::new(a[0].as_f64());
                if f == "set" {
                    e = CircleEmitter::default();
                    e.set_radius(n(&a[0]));
                }
                (num(e.radius()), emit_many(&e, &a[1], &a[2]))
            }
            ("cone", _) => {
                let mut e = ConeEmitter::new(a[0].as_f64());
                if f == "set" {
                    e = ConeEmitter::default();
                    e.set_angle(n(&a[0]));
                }
                (num(e.angle()), emit_many(&e, &a[1], &a[2]))
            }
            _ => {
                let mut e = SphereEmitter::new(a[0].as_f64());
                if f == "set" {
                    e = SphereEmitter::default();
                    e.set_radius(n(&a[0]));
                }
                (num(e.radius()), emit_many(&e, &a[1], &a[2]))
            }
        };
        ok(json!([property, emitted]))
    }
}

#[test]
fn box_emitter() {
    run("scene", "BoxEmitter", |_| EXACT, emitter_port("box"));
}

#[test]
fn circle_emitter() {
    run("scene", "CircleEmitter", |_| EXACT, emitter_port("circle"));
}

#[test]
fn cone_emitter() {
    run("scene", "ConeEmitter", |_| EXACT, emitter_port("cone"));
    assert_eq!(ConeEmitter::default().angle(), to_radians(30.0));
}

#[test]
fn sphere_emitter() {
    run("scene", "SphereEmitter", |_| EXACT, emitter_port("sphere"));
}

#[test]
fn particle_emitter() {
    // Through the trait: emit sets only the position and velocity.
    run(
        "scene",
        "ParticleEmitter",
        |_| EXACT,
        |_, a| {
            set_random_number_seed(u(&a[1]));
            let mut p = Particle::new(options(&json!({ "mass": 3, "life": 9, "startScale": 2, "imageSize": [4, 4] })));
            emitter(s(&a[0]), &Value::Null).emit(&mut p);
            ok(json!([true, true, state(&p)]))
        },
    );
}

#[test]
fn point_cloud_shading() {
    run(
        "scene",
        "PointCloudShading",
        |_| EXACT,
        |f, a| {
            if f == "isSupported" {
                let capabilities = ContextCapabilities {
                    draw_buffers: a[0] == true,
                    fragment_depth: a[1] == true,
                    ..Default::default()
                };
                return ok(json!(PointCloudShading::is_supported(&capabilities)));
            }
            let o = &a[0];
            let p = PointCloudShading::new(PointCloudShadingOptions {
                attenuation: o.get("attenuation").and_then(Value::as_bool),
                geometric_error_scale: o.get("geometricErrorScale").and_then(Value::as_f64),
                maximum_attenuation: o.get("maximumAttenuation").and_then(Value::as_f64),
                base_resolution: o.get("baseResolution").and_then(Value::as_f64),
                eye_dome_lighting: o.get("eyeDomeLighting").and_then(Value::as_bool),
                eye_dome_lighting_strength: o.get("eyeDomeLightingStrength").and_then(Value::as_f64),
                eye_dome_lighting_radius: o.get("eyeDomeLightingRadius").and_then(Value::as_f64),
                back_face_culling: o.get("backFaceCulling").and_then(Value::as_bool),
                normal_shading: o.get("normalShading").and_then(Value::as_bool),
            });
            ok(json!([
                p.attenuation,
                num(p.geometric_error_scale),
                p.maximum_attenuation.map(num),
                p.base_resolution.map(num),
                p.eye_dome_lighting,
                num(p.eye_dome_lighting_strength),
                num(p.eye_dome_lighting_radius),
                p.back_face_culling,
                p.normal_shading,
            ]))
        },
    );
}

#[test]
fn imagery_layer_feature_info() {
    run(
        "scene",
        "ImageryLayerFeatureInfo",
        |_| EXACT,
        |f, a| {
            let mut info = ImageryLayerFeatureInfo::new();
            match f {
                "new" => ok(json!([
                    info.name,
                    info.description,
                    info.position.map(|_| "position"),
                    info.data,
                    info.imagery_layer.map(|_| "layer")
                ])),
                "name" => {
                    info.configure_name_from_properties(&a[0]);
                    ok(info.name.unwrap_or(Value::Null))
                }
                _ => {
                    info.configure_description_from_properties(&a[0]);
                    ok(json!(info.description))
                }
            }
        },
    );
}

fn keyframe(k: &KeyframeNode<Value, &'static str>) -> Value {
    json!([
        k.spatial_node,
        k.keyframe,
        k.state as u8,
        k.content,
        k.megatexture_index,
        num(k.priority),
        k.high_priority_frame_number
    ])
}

#[test]
fn keyframe_node() {
    run(
        "scene",
        "KeyframeNode",
        |_| EXACT,
        |f, a| match f {
            "loadState" => {
                use KeyframeLoadState::*;
                let names = [
                    ("UNLOADED", Unloaded),
                    ("RECEIVING", Receiving),
                    ("PROCESSING", Processing),
                    ("LOADED", Loaded),
                    ("FAILED", Failed),
                    ("UNAVAILABLE", Unavailable),
                ];
                ok(Value::Object(names.iter().map(|(k, v)| (k.to_string(), json!(*v as u8))).collect::<Map<_, _>>()))
            }
            "lifecycle" => {
                let mut k = KeyframeNode::new(a[0].clone(), u(&a[1]) as usize);
                let created = keyframe(&k);
                k.state = KeyframeLoadState::Loaded;
                k.content = Some("content");
                k.megatexture_index = 5;
                k.priority = 12.5;
                k.high_priority_frame_number = 40;
                let loaded = keyframe(&k);
                k.unload();
                ok(json!([created, loaded, keyframe(&k)]))
            }
            _ => {
                let mut x = KeyframeNode::<Value, ()>::new(json!({}), u(&a[2]) as usize);
                let mut y = KeyframeNode::<Value, ()>::new(json!({}), u(&a[3]) as usize);
                x.priority = n(&a[0]);
                y.priority = n(&a[1]);
                ok(json!([
                    num(KeyframeNode::priority_comparator(&x, &y)),
                    num(KeyframeNode::priority_comparator(&y, &x)),
                    num(KeyframeNode::search_comparator(&x, &y))
                ]))
            }
        },
    );
}

struct Layer(Value);

impl ImageryLayerSettings for Layer {
    fn show(&self) -> bool {
        self.0["show"] == true
    }
    fn alpha(&self) -> f64 {
        n(&self.0["alpha"])
    }
    fn brightness(&self) -> f64 {
        n(&self.0["brightness"])
    }
    fn contrast(&self) -> f64 {
        n(&self.0["contrast"])
    }
    fn hue(&self) -> f64 {
        n(&self.0["hue"])
    }
    fn saturation(&self) -> f64 {
        n(&self.0["saturation"])
    }
    fn gamma(&self) -> f64 {
        n(&self.0["gamma"])
    }
    fn color_to_alpha(&self) -> Option<Color> {
        (!self.0["colorToAlpha"].is_null()).then(|| color_of(&self.0["colorToAlpha"]))
    }
}

#[test]
fn imagery_configuration() {
    run(
        "scene",
        "ImageryConfiguration",
        |_| EXACT,
        |_, a| {
            let c = ImageryConfiguration::new(&Layer(a[0].clone()));
            ok(json!([
                c.show,
                num(c.alpha),
                num(c.brightness),
                num(c.contrast),
                num(c.hue),
                num(c.saturation),
                num(c.gamma),
                c.color_to_alpha.map(rgba)
            ]))
        },
    );
}

#[derive(Debug)]
struct Image {
    #[allow(dead_code)]
    src: String,
}

#[test]
fn discard_empty_tile_image_policy() {
    run(
        "scene",
        "DiscardEmptyTileImagePolicy",
        |_| EXACT,
        |_, _| {
            let empty = Rc::new(Image { src: EMPTY_IMAGE_URI.into() });
            let a = DiscardEmptyTileImagePolicy::new(empty.clone());
            let b = DiscardEmptyTileImagePolicy::new(empty.clone());
            let other = Rc::new(Image { src: String::new() });
            let copy = Rc::new(Image { src: EMPTY_IMAGE_URI.into() });
            ok(json!({
                "src": EMPTY_IMAGE_URI,
                "shared": Rc::ptr_eq(a.empty_image(), b.empty_image()),
                "ready": [a.is_ready(), b.is_ready()],
                "discardEmpty": [a.should_discard_image(a.empty_image()), b.should_discard_image(&empty)],
                "discardOther": a.should_discard_image(&other),
                "discardCopy": a.should_discard_image(&copy),
            }))
        },
    );
}
