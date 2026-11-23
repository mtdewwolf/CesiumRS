//! Port of CesiumJS `Scene/I3SDecoder`.

use crate::core::cartographic::Cartographic;
use crate::core::ellipsoid::Ellipsoid;
use crate::core::math::PI_OVER_TWO;
use crate::core::matrix3::Matrix3;
use crate::scene::i3s_node::I3SNode;
use crate::workers::decode_i3s::{decode_i3s, I3sDecodeParameters, I3sGeoid, I3sMeshData, I3sSymbology};
use serde_json::Value;

/// Port of `I3SDecoder`: transcodes I3S binary geometry into the glTF
/// document the native `decode_i3s` worker produces. Returns `None` when the
/// task cannot be scheduled this frame, like Cesium's `undefined`.
pub fn decode(
    url: &str,
    default_geometry_schema: &Value,
    node: &I3SNode,
    feature_data: Option<&crate::scene::i3s_feature::I3SFeature>,
    symbology_data: Option<&Value>,
) -> Option<I3sMeshData> {
    let geometry_data = node.geometry_data().first()?;
    let parent_data = node.data()?;
    let parent_inverse_rotation_matrix = node.inverse_rotation_matrix()?;
    let (longitude, latitude, height) = if let Some(obb) = parent_data.get("obb") {
        let center = obb.get("center").and_then(Value::as_array)?;
        (
            center.first().and_then(Value::as_f64).unwrap_or_default(),
            center.get(1).and_then(Value::as_f64).unwrap_or_default(),
            center.get(2).and_then(Value::as_f64).unwrap_or_default(),
        )
    } else if let Some(mbs) = parent_data.get("mbs").and_then(Value::as_array) {
        (
            mbs.first().and_then(Value::as_f64).unwrap_or_default(),
            mbs.get(1).and_then(Value::as_f64).unwrap_or_default(),
            mbs.get(2).and_then(Value::as_f64).unwrap_or_default(),
        )
    } else {
        (0.0, 0.0, 0.0)
    };

    let axis_flip_rotation = Matrix3::from_rotation_x(-PI_OVER_TWO);
    let parent_rotation = axis_flip_rotation.multiply(&parent_inverse_rotation_matrix);

    let cartographic_center = Cartographic::from_degrees(longitude, latitude, height);
    let cartesian_center = Ellipsoid::WGS84.cartographic_to_cartesian(&cartographic_center);

    let provider = node.data_provider()?;
    let geoid_data_list: Vec<I3sGeoid> = provider
        .geoid_data_list()
        .unwrap_or_default()
        .into_iter()
        .map(I3sGeoid::from)
        .collect();
    let _ = feature_data; // The native worker decodes feature data from the binary payload.

    let schema_list = |key: &str| {
        default_geometry_schema
            .get(key)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
    };
    let parameters = I3sDecodeParameters {
        binary_data: geometry_data.data().cloned().unwrap_or_default(),
        ordering: schema_list("ordering"),
        feature_attribute_order: schema_list("featureAttributeOrder"),
        buffer_attributes: geometry_data.geometry_buffer_info.as_ref().map(|info| info.attributes.clone()),
        geoid_data_list,
        cartographic_center: Some(cartographic_center),
        cartesian_center: Some(cartesian_center),
        parent_rotation: Some(parent_rotation),
        ellipsoid_radii_square: Some(Ellipsoid::WGS84.radii_squared),
        url: Some(url.to_string()),
        enable_features: provider.show_features(),
        split_geometry_by_color_transparency: provider.adjust_material_alpha_mode(),
        calculate_normals: provider.calculate_normals(),
        symbology: symbology_data.map(symbology_to_entries),
    };

    decode_i3s(&parameters).ok()
}

/// Converts the symbology object produced by
/// [`I3SSymbology::get_symbology`](crate::scene::i3s_symbology::I3SSymbology::get_symbology)
/// into the worker's `I3sSymbology` entries: property names in insertion
/// order, where `true` means the `edges` are defined.
fn symbology_to_entries(symbology: &Value) -> I3sSymbology {
    I3sSymbology {
        entries: symbology
            .as_object()
            .map(|object| {
                object
                    .iter()
                    .map(|(name, value)| (name.clone(), value.get("edges").is_some_and(|e| !e.is_null())))
                    .collect()
            })
            .unwrap_or_default(),
    }
}
