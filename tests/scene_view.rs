mod common;
use cesium_rs::{core::RuntimeError, renderer::*, scene::*};
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
