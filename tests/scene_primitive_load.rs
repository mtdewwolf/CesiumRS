mod common;
use cesium_rs::{core::*, renderer::*, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
    bound: Rc<Cell<u32>>,
    buffers: Rc<RefCell<std::collections::HashMap<u32, Vec<u8>>>>,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "bindBuffer" {
            if let Some(GlValue::Object(id)) = args.get(1) {
                self.bound.set(*id);
            }
        }
        if name == "bufferData" {
            if let Some(GlValue::Bytes(b)) = args.get(1) {
                self.buffers.borrow_mut().insert(self.bound.get(), b.clone());
            }
        }
        if name == "getParameter" {
            if matches!(args.first(),Some(GlValue::Number(v)) if [webgl_constants::ALIASED_LINE_WIDTH_RANGE,webgl_constants::ALIASED_POINT_SIZE_RANGE,webgl_constants::MAX_VIEWPORT_DIMS].contains(&(*v as u32)))
            {
                return Ok(GlValue::Numbers(vec![1., 4096.]));
            }
            return Ok(GlValue::Number(4096.));
        }
        Ok(GlValue::Null)
    }
}
fn nums(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(n).collect()
}
fn array(a: &TypedArray) -> Value {
    a.to_f64_vec().into_iter().map(num).collect()
}
fn summary<G: Gl>(a: &model_components::Attribute<G>, uploaded: &[Vec<u8>], slot: &mut usize) -> Value {
    let buffer = if a.buffer.is_some() {
        let b = json!(uploaded[*slot]);
        *slot += 1;
        b
    } else {
        Value::Null
    };
    json!({"array":a.typed_array.as_ref().map(array),"count":a.count,"buffer":buffer})
}
#[test]
fn primitive_load_matches_cesium() {
    run(
        "scene",
        "primitive-load",
        |_| EXACT,
        |f, args| {
            let a = &args[0];
            let actual = match f {
                "mesh" => get_mesh_primitives(a).unwrap().unwrap_or(Value::Null),
                "outline" => {
                    let v = nums(&a["triangles"]);
                    let t = match a["kind"].as_str().unwrap() {
                        "u8" => TypedArray::Uint8(v.iter().map(|x| *x as u8).collect()),
                        "u16" => TypedArray::Uint16(v.iter().map(|x| *x as u16).collect()),
                        _ => TypedArray::Uint32(v.iter().map(|x| *x as u32).collect()),
                    };
                    let count = a["count"].as_u64().unwrap() as usize;
                    let g = model::PrimitiveOutlineGenerator::new(t, &nums(&a["edges"]), count);
                    let attr = TypedArray::Float32((0..count * 2).map(|i| i as f32 / 8.).collect());
                    let kind = match g.updated_triangle_indices() {
                        TypedArray::Uint8(_) => "Uint8Array",
                        TypedArray::Uint16(_) => "Uint16Array",
                        _ => "Uint32Array",
                    };
                    json!({"indices":array(g.updated_triangle_indices()),"kind":kind,"coordinates":g.outline_coordinates().iter().map(|v|num(f64::from(*v))).collect::<Vec<_>>(),"attribute":array(&g.update_attribute(&attr).unwrap())})
                }
                "plan" => {
                    let host = Host::default();
                    let ctx = Context::new_webgl2(host.clone());
                    let mut p = model_components::Primitive::<Host>::default();
                    p.attributes.push(model_components::Attribute {
                        count: Some(4),
                        typed_array: Some(TypedArray::Float32(vec![1., 2., 3., 4., 5., 6., 7., 8.])),
                        ..Default::default()
                    });
                    p.indices = Some(model_components::Indices {
                        typed_array: Some(TypedArray::Uint8(vec![0, 1, 2, 0, 2, 3])),
                        index_datatype: Some(IndexDatatype::UnsignedByte),
                        ..Default::default()
                    });
                    let mut plan = PrimitiveLoadPlan {
                        needs_outlines: a["outline"].as_bool().unwrap(),
                        needs_gaussian_splats: a["splat"].as_bool().unwrap(),
                        outline_indices: Some(vec![0., 1., 1., 2., 2., 3., 3., 0.]),
                        ..Default::default()
                    };
                    let upload = a["upload"].as_bool().unwrap();
                    let retain = a["retain"].as_bool().unwrap();
                    plan.attribute_plans.push(AttributeLoadPlan {
                        attribute: PlannedAttribute::Attribute(0),
                        load_buffer: upload,
                        load_typed_array: retain,
                    });
                    plan.indices_plan = Some(IndicesLoadPlan { load_buffer: upload, load_typed_array: retain });
                    plan.post_process(&mut p, &ctx).unwrap();
                    let mut uploaded = host.buffers.borrow().iter().map(|(i, b)| (*i, b.clone())).collect::<Vec<_>>();
                    uploaded.sort_by_key(|(i, _)| *i);
                    let uploaded = uploaded.into_iter().map(|(_, b)| b).collect::<Vec<_>>();
                    let mut slot = 0;
                    let attribute = summary(&p.attributes[0], &uploaded, &mut slot);
                    let outline = p.outline_coordinates.as_ref().map(|a| summary(a, &uploaded, &mut slot));
                    let indices = p.indices.as_ref().unwrap();
                    let buffer = if indices.buffer.is_some() { json!(uploaded[slot]) } else { Value::Null };
                    json!({"attribute":attribute,"outline":outline,"indices":{"array":indices.typed_array.as_ref().map(array),"count":indices.count,"buffer":buffer},"plans":plan.attribute_plans.iter().map(|p|[p.load_buffer,p.load_typed_array]).collect::<Vec<_>>()})
                }
                _ => unreachable!(),
            };
            Some(Out::Ok(actual))
        },
    );
}

#[test]
fn outline_lookup_texture_is_cached_by_context() {
    let context = Context::new_webgl2(Host::default());
    let first = model::PrimitiveOutlineGenerator::create_texture(&context).unwrap();
    let second = model::PrimitiveOutlineGenerator::create_texture(&context).unwrap();
    assert!(Rc::ptr_eq(&first, &second));
    assert_eq!(first.width(), 4096);
    assert_eq!(first.height(), 1);
    assert_eq!(first.sampler().minification_filter, TextureMinificationFilter::LinearMipmapLinear);
    let other = Context::new_webgl2(Host::default());
    let third = model::PrimitiveOutlineGenerator::create_texture(&other).unwrap();
    assert!(!Rc::ptr_eq(&first, &third));
}
