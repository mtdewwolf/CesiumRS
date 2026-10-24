//! The NGA_gpm_local data classes and GltfGpmLoader against CesiumJS
//! (`tools/cesium/fixtures/scene-gpm.mjs`).
mod common;
use cesium_rs::scene::model::extensions::gpm::{self, *};
use common::*;
use serde_json::{json, Value};

fn direct(p: &AnchorPointDirect) -> Value {
    json!({"position": p.position.enc(), "adjustmentParams": p.adjustment_params.enc()})
}

fn indirect(p: &AnchorPointIndirect) -> Value {
    json!({"position": p.position.enc(), "adjustmentParams": p.adjustment_params.enc(), "covarianceMatrix": p.covariance_matrix.enc()})
}

fn spdcf(s: &Spdcf) -> Value {
    json!({"A": s.a.enc(), "alpha": s.alpha.enc(), "beta": s.beta.enc(), "T": s.t.enc()})
}

fn group(g: &CorrelationGroup) -> Value {
    json!({"groupFlags": g.group_flags, "rotationThetas": g.rotation_thetas.enc(), "params": g.params.iter().map(spdcf).collect::<Vec<_>>()})
}

fn local(l: &GltfGpmLocal) -> Value {
    json!({
        "storageType": l.storage_type().as_str(),
        "anchorPointsDirect": l.anchor_points_direct().map(|a| a.iter().map(direct).collect::<Vec<_>>()),
        "anchorPointsIndirect": l.anchor_points_indirect().map(|a| a.iter().map(indirect).collect::<Vec<_>>()),
        "intraTileCorrelationGroups": l.intra_tile_correlation_groups().map(|g| g.iter().map(group).collect::<Vec<_>>()),
        "covarianceDirect": l.covariance_direct().map(|m| m.enc()),
    })
}

fn metadata(m: &PpeMetadata) -> Value {
    json!({"min": m.min, "max": m.max, "source": m.source})
}

fn texture(t: &PpeTexture) -> Value {
    json!({"traits": metadata(&t.traits), "noData": t.no_data, "offset": t.offset, "scale": t.scale, "index": t.index, "texCoord": t.tex_coord})
}

fn metadata_of(v: &Value) -> PpeMetadata {
    PpeMetadata { min: opt(&v[0]), max: opt(&v[1]), source: s(&v[2]).into() }
}

fn spdcf_of(v: &Value) -> Spdcf {
    Spdcf { a: n(&v[0]), alpha: n(&v[1]), beta: n(&v[2]), t: n(&v[3]) }
}

#[test]
fn data_classes() {
    run("scene", "AnchorPointDirect", |_| EXACT, |_, a| {
        ok(direct(&AnchorPointDirect { position: c3(&a[0]), adjustment_params: c3(&a[1]) }))
    });
    run("scene", "AnchorPointIndirect", |_| EXACT, |_, a| {
        ok(indirect(&AnchorPointIndirect { position: c3(&a[0]), adjustment_params: c3(&a[1]), covariance_matrix: m3(&a[2]) }))
    });
    run("scene", "Spdcf", |_| EXACT, |_, a| ok(spdcf(&Spdcf { a: n(&a[0]), alpha: n(&a[1]), beta: n(&a[2]), t: n(&a[3]) })));
    run("scene", "CorrelationGroup", |_| EXACT, |_, a| {
        let params = a[2].as_array().unwrap().iter().map(spdcf_of).collect();
        ok(group(&CorrelationGroup { group_flags: a[0].clone(), rotation_thetas: c3(&a[1]), params }))
    });
    run("scene", "PpeSource", |_| EXACT, |_, _| {
        ok(Value::Object(PpeSource::ALL.iter().map(|p| (p.name().to_string(), json!(p.as_str()))).collect()))
    });
    run("scene", "PpeMetadata", |_| EXACT, |_, a| ok(metadata(&PpeMetadata { min: opt(&a[0]), max: opt(&a[1]), source: s(&a[2]).into() })));
    run("scene", "PpeTexture", |_| EXACT, |_, a| {
        ok(texture(&PpeTexture {
            traits: metadata_of(&a[0]),
            no_data: opt(&a[1]),
            offset: opt(&a[2]),
            scale: opt(&a[3]),
            index: u(&a[4]) as usize,
            tex_coord: opt(&a[5]).map(|t| t as u32),
        }))
    });
    run("scene", "MeshPrimitiveGpmLocal", |_| EXACT, |_, a| {
        let ppe_textures = a[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| PpeTexture {
                traits: metadata_of(t),
                no_data: None,
                offset: None,
                scale: None,
                index: u(&t[3]) as usize,
                tex_coord: None,
            })
            .collect();
        ok(MeshPrimitiveGpmLocal { ppe_textures }.ppe_textures.iter().map(texture).collect::<Vec<_>>())
    });
}

#[test]
fn gltf_gpm_local() {
    run("scene", "GltfGpmLocal", |_| EXACT, |f, a| {
        let built = match f {
            "direct" => GltfGpmLocal::direct(
                a[0].as_array().map_or(Vec::new(), |p| {
                    if p.is_empty() {
                        Vec::new()
                    } else {
                        vec![AnchorPointDirect { position: c3(&p[0]), adjustment_params: c3(&p[1]) }]
                    }
                }),
                m3(&a[1]),
            ),
            _ => GltfGpmLocal::indirect(
                vec![AnchorPointIndirect { position: c3(&a[0][0]), adjustment_params: c3(&a[0][1]), covariance_matrix: m3(&a[0][2]) }],
                vec![CorrelationGroup { group_flags: a[1][0].clone(), rotation_thetas: c3(&a[1][1]), params: Vec::new() }],
            ),
        };
        ok(local(&built))
    });
}

#[test]
fn gltf_gpm_loader() {
    run("scene", "GltfGpmLoader", |_| EXACT, |f, a| {
        let result = match f {
            "load" => gpm::gltf_gpm_loader::load(&a[0]),
            "loadDirect" => gpm::gltf_gpm_loader::load_direct(&a[0]),
            _ => gpm::gltf_gpm_loader::load_indirect(&a[0]),
        };
        Some(match result {
            Ok(l) => Out::Ok(local(&l)),
            Err(e) => Out::Err(e.message),
        })
    });
}
