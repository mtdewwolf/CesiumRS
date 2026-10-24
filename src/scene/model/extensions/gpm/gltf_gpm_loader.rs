//! CesiumJS `GltfGpmLoader`: reads a glTF's `NGA_gpm_local` extension JSON
//! into a [`GltfGpmLocal`].
//!
//! JSON is read with JavaScript's semantics: `Cartesian3.fromArray` keeps a
//! missing element undefined (NaN here), the covariance `Matrix3` constructor
//! turns a missing or null element into 0, iterating a missing array or
//! reading a property of null is the `TypeError` JavaScript throws, and the
//! parameters' debug-build range checks are not ported.

use super::{AnchorPointDirect, AnchorPointIndirect, CorrelationGroup, GltfGpmLocal, Spdcf, StorageType};
use crate::core::{Cartesian3, Matrix3, RuntimeError};
use crate::scene::gltf_pipeline::js;
use serde_json::Value;

fn type_error(message: String) -> RuntimeError {
    RuntimeError::new(format!("TypeError: {message}"))
}

/// `for (const item of value)`.
fn items<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a [Value], RuntimeError> {
    match value {
        Some(Value::Array(items)) => Ok(items),
        _ => Err(type_error(format!("{name} is not iterable"))),
    }
}

/// `object.property`, where reading a property of null or undefined throws.
fn field<'a>(object: &'a Value, property: &str) -> Result<Option<&'a Value>, RuntimeError> {
    if object.is_null() {
        return Err(type_error(format!("Cannot read properties of null (reading '{property}')")));
    }
    Ok(js::get(Some(object), property))
}

/// `Cartesian3.fromArray(array, 0)`: undefined elements stay undefined (NaN).
fn cartesian(array: Option<&Value>) -> Result<Cartesian3, RuntimeError> {
    let array = array
        .filter(|v| !v.is_null())
        .ok_or_else(|| type_error("Cannot read properties of undefined (reading '0')".into()))?;
    let element = |i: usize| js::get(Some(array), &i.to_string()).map_or(f64::NAN, |v| js::to_number(Some(v)));
    Ok(Cartesian3::new(element(0), element(1), element(2)))
}

fn number(object: &Value, property: &str) -> Result<f64, RuntimeError> {
    Ok(field(object, property)?.map_or(f64::NAN, |v| js::to_number(Some(v))))
}

/// `createCovarianceMatrixFromUpperTriangle`: the symmetric matrix of
/// `[c00, c01, c11, c02, c12, c22]`.
fn covariance(array: Option<&Value>) -> Result<Matrix3, RuntimeError> {
    let array = array
        .filter(|v| !v.is_null())
        .ok_or_else(|| type_error("Cannot read properties of null (reading '0')".into()))?;
    // The Matrix3 constructor defaults undefined and null elements to 0.
    let e = |i: usize| {
        js::get(Some(array), &i.to_string()).filter(|v| !v.is_null()).map_or(0.0, |v| js::to_number(Some(v)))
    };
    Ok(Matrix3::new(e(0), e(1), e(3), e(1), e(2), e(4), e(3), e(4), e(5)))
}

fn anchor_point_direct(json: &Value) -> Result<AnchorPointDirect, RuntimeError> {
    Ok(AnchorPointDirect {
        position: cartesian(field(json, "position")?)?,
        adjustment_params: cartesian(field(json, "adjustmentParams")?)?,
    })
}

fn anchor_point_indirect(json: &Value) -> Result<AnchorPointIndirect, RuntimeError> {
    Ok(AnchorPointIndirect {
        position: cartesian(field(json, "position")?)?,
        adjustment_params: cartesian(field(json, "adjustmentParams")?)?,
        covariance_matrix: covariance(field(json, "covarianceMatrix")?)?,
    })
}

fn correlation_group(json: &Value) -> Result<CorrelationGroup, RuntimeError> {
    let group_flags = field(json, "groupFlags")?.cloned().unwrap_or(Value::Null);
    let rotation_thetas = cartesian(field(json, "rotationThetas")?)?;
    let params = items(field(json, "params")?, "correlationGroupJson.params")?
        .iter()
        .map(|p| {
            Ok(Spdcf { a: number(p, "A")?, alpha: number(p, "alpha")?, beta: number(p, "beta")?, t: number(p, "T")? })
        })
        .collect::<Result<_, RuntimeError>>()?;
    Ok(CorrelationGroup { group_flags, rotation_thetas, params })
}

/// `GltfGpmLoader.load`: direct or indirect storage by `storageType`.
pub fn load(json: &Value) -> Result<GltfGpmLocal, RuntimeError> {
    let storage_type = field(json, "storageType")?;
    match storage_type.and_then(Value::as_str).and_then(StorageType::from_str_value) {
        Some(StorageType::Direct) => load_direct(json),
        Some(StorageType::Indirect) => load_indirect(json),
        None => Err(RuntimeError::new(format!(
            "Invalid storage type in NGA_gpm_local - expected 'Direct' or 'Indirect', but found {}",
            storage_type.map_or_else(|| "undefined".into(), js::key)
        ))),
    }
}

/// `GltfGpmLoader.loadDirect`.
pub fn load_direct(json: &Value) -> Result<GltfGpmLocal, RuntimeError> {
    let anchor_points = items(field(json, "anchorPointsDirect")?, "anchorPointsDirectJson")?
        .iter()
        .map(anchor_point_direct)
        .collect::<Result<_, _>>()?;
    let covariance = covariance(field(json, "covarianceDirectUpperTriangle")?)?;
    Ok(GltfGpmLocal::direct(anchor_points, covariance))
}

/// `GltfGpmLoader.loadIndirect`.
pub fn load_indirect(json: &Value) -> Result<GltfGpmLocal, RuntimeError> {
    let anchor_points = items(field(json, "anchorPointsIndirect")?, "anchorPointsIndirectJson")?
        .iter()
        .map(anchor_point_indirect)
        .collect::<Result<_, _>>()?;
    let groups = items(field(json, "intraTileCorrelationGroups")?, "intraTileCorrelationGroupsJson")?
        .iter()
        .map(correlation_group)
        .collect::<Result<_, _>>()?;
    Ok(GltfGpmLocal::indirect(anchor_points, groups))
}
