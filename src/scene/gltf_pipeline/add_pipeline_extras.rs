//! CesiumJS `addPipelineExtras`: gives the glTF, its shaders, buffers and
//! images an `extras._pipeline` object.

use super::for_each::Collection;
use super::{for_each, js, Result};
use serde_json::{Map, Value};

const PIPELINE_COLLECTIONS: [Collection; 3] =
    [Collection::Techniques("shaders"), Collection::TopLevel("buffers"), Collection::TopLevel("images")];

pub fn add_pipeline_extras(gltf: &mut Value) -> Result<()> {
    for which in PIPELINE_COLLECTIONS {
        for id in for_each::ids(gltf, which) {
            if let Some(object) = for_each::get_mut(gltf, which, &id) {
                add_extras(object)?;
            }
        }
    }
    add_extras(gltf)
}

fn add_extras(object: &mut Value) -> Result<()> {
    if !js::defined(object.get("extras")) {
        js::set(object, "extras", Value::Object(Map::new()))?;
    }
    let extras = js::object_mut(object.get_mut("extras"), "_pipeline")?;
    if !js::defined(extras.get("_pipeline")) {
        js::set(extras, "_pipeline", Value::Object(Map::new()))?;
    }
    Ok(())
}

/// The collections whose elements carry `extras._pipeline`.
pub(crate) fn pipeline_collections() -> [Collection; 3] {
    PIPELINE_COLLECTIONS
}
