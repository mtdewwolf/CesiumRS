//! External tilesets and implicit subtree transcoding into the tile arena.
use super::{
    cesium_3d_tileset::{array, Cesium3DTileset, TileId},
    *,
};
use crate::core::{resource::DerivedResourceOptions, *};
use serde_json::{json, Value};
use std::rc::Rc;

fn runtime(e: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(e.to_string())
}
pub(super) fn metadata_json(v: &MetadataValue) -> Value {
    use MetadataValue::*;
    match v {
        Null => Value::Null,
        Boolean(v) => json!(v),
        Number(v) => json!(v),
        BigInt(v) => json!(v.to_string()),
        String(v) => json!(v),
        Array(a) => Value::Array(a.iter().map(metadata_json).collect()),
        Object(v) => v.clone(),
        Cartesian2(v) => json!([v.x, v.y]),
        Cartesian3(v) => json!([v.x, v.y, v.z]),
        Cartesian4(v) => json!([v.x, v.y, v.z, v.w]),
        Matrix2(v) => json!(v.0),
        Matrix3(v) => json!(v.0),
        Matrix4(v) => json!(v.0),
    }
}
pub(super) fn semantic_volume(
    metadata: &dyn MetadataEntity,
    prefix: bounding_volume_semantics::BoundingVolumePrefix,
) -> Result<Option<Value>, RuntimeError> {
    use bounding_volume_semantics::SemanticBoundingVolume::*;
    Ok(bounding_volume_semantics::parse_bounding_volume_semantic(prefix, metadata).map_err(runtime)?.map(|v| match v {
        Box(v) => json!({"box":metadata_json(&v)}),
        Region(v) => json!({"region":metadata_json(&v)}),
        Sphere(v) => json!({"sphere":metadata_json(&v)}),
    }))
}
struct MetadataBounds {
    volume: Option<Value>,
    minimum: Option<f64>,
    maximum: Option<f64>,
}
impl MetadataBounds {
    fn read(
        metadata: Option<&dyn MetadataEntity>,
        prefix: bounding_volume_semantics::BoundingVolumePrefix,
    ) -> Result<Self, RuntimeError> {
        let Some(metadata) = metadata else { return Ok(Self { volume: None, minimum: None, maximum: None }) };
        let values =
            bounding_volume_semantics::parse_all_bounding_volume_semantics(prefix, metadata).map_err(runtime)?;
        Ok(Self {
            volume: semantic_volume(metadata, prefix)?,
            minimum: values.minimum_height.map(|v| v.to_number()),
            maximum: values.maximum_height.map(|v| v.to_number()),
        })
    }
    fn can_update(&self, volume: Option<&Value>) -> bool {
        (self.minimum.is_some() || self.maximum.is_some())
            && volume.is_some_and(|v| {
                v.get("region").is_some() || v.pointer("/extensions/3DTILES_bounding_volume_S2").is_some()
            })
    }
    fn update(&self, volume: &mut Value) {
        if let Some(region) = volume.get_mut("region").and_then(Value::as_array_mut) {
            if let Some(h) = self.minimum {
                region[4] = json!(h);
            }
            if let Some(h) = self.maximum {
                region[5] = json!(h);
            }
        }
        if let Some(s2) = volume.pointer_mut("/extensions/3DTILES_bounding_volume_S2") {
            if let Some(h) = self.minimum {
                s2["minimumHeight"] = json!(h);
            }
            if let Some(h) = self.maximum {
                s2["maximumHeight"] = json!(h);
            }
        }
    }
}
fn template(resource: &Resource, c: ImplicitTileCoordinates) -> Result<Resource, RuntimeError> {
    resource
        .get_derived_resource(DerivedResourceOptions {
            template_values: c.get_template_values().into_iter().map(|(k, v)| (k.into(), v.to_string())).collect(),
            ..Default::default()
        })
        .ok_or_else(|| RuntimeError::new("invalid implicit URI template"))
}
pub fn derive_implicit_volume(
    root: &Value,
    c: ImplicitTileCoordinates,
    parent: Option<(&Value, u32)>,
) -> Result<Value, RuntimeError> {
    let scale = 2f64.powi(-(c.level as i32));
    if let Some(s2) = root.pointer("/extensions/3DTILES_bounding_volume_S2") {
        let cell = S2Cell::from_token(s2["token"].as_str().unwrap_or(""))
            .ok_or_else(|| RuntimeError::new("invalid implicit S2 token"))?;
        let face = (cell.cell_id() >> 61) as u8;
        let token = if c.level == 0 {
            s2["token"].as_str().unwrap().to_owned()
        } else {
            let (x, y) = if face.is_multiple_of(2) { (c.x, c.y) } else { (c.y, c.x) };
            let position = HilbertOrder::encode_2d(c.level, x as u64, y as u64)
                .ok_or_else(|| RuntimeError::new("invalid implicit S2 coordinates"))?;
            let child = S2Cell::from_face_position_level(face, position as u64, c.level)
                .ok_or_else(|| RuntimeError::new("invalid implicit S2 level"))?;
            S2Cell::get_token_from_id(child.cell_id())
        };
        let (min, max) = if let Some((p, index)) = parent.filter(|_| c.z.is_some()) {
            let p = p.pointer("/extensions/3DTILES_bounding_volume_S2").unwrap_or(s2);
            let min = p["minimumHeight"].as_f64().unwrap_or(0.);
            let max = p["maximumHeight"].as_f64().unwrap_or(0.);
            let mid = (min + max) * 0.5;
            if index < 4 {
                (min, mid)
            } else {
                (mid, max)
            }
        } else {
            let min = s2["minimumHeight"].as_f64().unwrap_or(0.);
            let max = s2["maximumHeight"].as_f64().unwrap_or(0.);
            if let Some(z) = c.z {
                let size = (max - min) * scale;
                (min + z as f64 * size, min + (z + 1) as f64 * size)
            } else {
                (min, max)
            }
        };
        return Ok(
            json!({"extensions":{"3DTILES_bounding_volume_S2":{"token":token,"minimumHeight":min,"maximumHeight":max}}}),
        );
    }
    if let Some(region) = root.get("region") {
        let [w, s, e, n, min, max] = array::<6>(region)?;
        if c.level == 0 {
            return Ok(json!({"region":[w,s,e,n,min,max]}));
        }
        let rectangle = Rectangle::new(w, s, e, n);
        let width = scale * rectangle.width();
        let height = scale * rectangle.height();
        let w = math::negative_pi_to_pi(w + c.x as f64 * width);
        let e = math::negative_pi_to_pi(w + width);
        let s = math::negative_pi_to_pi(s + c.y as f64 * height);
        let n = math::negative_pi_to_pi(s + height);
        let (min, max) = if let Some(z) = c.z {
            let size = (max - min) * scale;
            (min + z as f64 * size, min + (z + 1) as f64 * size)
        } else {
            (min, max)
        };
        return Ok(json!({"region":[w,s,e,n,min,max]}));
    }
    let a = array::<12>(&root["box"])?;
    if c.level == 0 {
        return Ok(root.clone());
    }
    let axes = Matrix3::from_array(&a[3..], 0);
    let p = Cartesian3::new(
        -1. + (2. * c.x as f64 + 1.) * scale,
        -1. + (2. * c.y as f64 + 1.) * scale,
        c.z.map_or(0., |z| -1. + (2. * z as f64 + 1.) * scale),
    );
    let center = Cartesian3::new(a[0], a[1], a[2]).add(axes.multiply_by_vector(p));
    let mut half = axes;
    half = half
        .set_column(0, axes.get_column(0).multiply_by_scalar(scale))
        .set_column(1, axes.get_column(1).multiply_by_scalar(scale));
    if c.z.is_some() {
        half = half.set_column(2, axes.get_column(2).multiply_by_scalar(scale));
    }
    let mut values = vec![center.x, center.y, center.z];
    values.extend(half.0);
    Ok(json!({"box":values}))
}
impl Cesium3DTileset {
    pub(super) fn expand_content(&mut self, id: TileId, expansion: TileExpansion) -> Result<(), RuntimeError> {
        match expansion {
            TileExpansion::External { resource, json } => {
                self.load_tileset(resource, &json, Some(id))?;
                self.tiles[id].has_external_content = true;
            }
            TileExpansion::Implicit { subtree, description, coordinates } => {
                self.expand_subtree(id, Rc::from(subtree), *description, coordinates)?;
                self.tiles[id].has_implicit_content = true;
            }
        }
        Ok(())
    }
    fn implicit_tile(
        &mut self,
        parent: TileId,
        description: &ImplicitTileset,
        subtree: &Rc<ImplicitSubtree>,
        c: ImplicitTileCoordinates,
        index: usize,
        child_index: u32,
    ) -> Result<TileId, RuntimeError> {
        use bounding_volume_semantics::BoundingVolumePrefix::{Content, Tile};
        let metadata = subtree.get_tile_metadata_view(&c).map_err(runtime)?;
        let bounds = MetadataBounds::read(metadata.as_ref().map(|m| m as &dyn MetadataEntity), Tile)?;
        let mut volume = match bounds.volume.as_ref() {
            Some(volume)
                if bounds.can_update(Some(volume)) || !bounds.can_update(Some(&description.bounding_volume)) =>
            {
                volume.clone()
            }
            _ => {
                if index == 0 && description.bounding_volume.pointer("/extensions/3DTILES_bounding_volume_S2").is_some()
                {
                    self.tiles[parent].source_bounds.clone()
                } else {
                    derive_implicit_volume(
                        &description.bounding_volume,
                        c,
                        Some((&self.tiles[parent].source_bounds, child_index)),
                    )?
                }
            }
        };
        bounds.update(&mut volume);
        let mut contents = Vec::new();
        for i in 0..description.content_count {
            if !subtree.content_is_available_at_index(index, Some(i)).map_err(runtime)? {
                continue;
            }
            let mut content = description.content_headers[i].clone();
            content["uri"] = json!(template(&description.content_uri_templates[i], c)?.url());
            let metadata = if subtree.content_property_table_jsons().is_empty() {
                None
            } else {
                subtree.get_content_metadata_view(&c, i).map_err(runtime)?
            };
            let bounds = MetadataBounds::read(metadata.as_ref().map(|m| m as &dyn MetadataEntity), Content)?;
            let mut content_volume = bounds.volume.clone();
            if bounds.can_update(content_volume.as_ref()) {
                bounds.update(content_volume.as_mut().unwrap());
            } else if bounds.can_update(Some(&volume)) {
                let mut v = volume.clone();
                bounds.update(&mut v);
                content_volume = Some(v);
            }
            if let Some(v) = content_volume {
                content["boundingVolume"] = v;
            }
            contents.push(content);
        }
        let error = metadata
            .as_ref()
            .map(|m| m.get_property_by_semantic("TILE_GEOMETRIC_ERROR"))
            .transpose()
            .map_err(runtime)?
            .flatten()
            .map_or(description.geometric_error.unwrap_or(f64::NAN) / 2f64.powi(c.level as i32), |v| v.to_number());
        let mut header = description.tile_header.clone();
        let h = header.as_object_mut().ok_or_else(|| RuntimeError::new("invalid implicit tile template"))?;
        h.remove("transform");
        h.remove("metadata");
        h.insert("boundingVolume".into(), volume);
        h.insert("geometricError".into(), json!(error));
        h.insert("contents".into(), json!(contents));
        if let Some(refine) = &description.refine {
            h.insert("refine".into(), json!(refine));
        }
        let tile = self.make_tile(&description.base_resource, &header, Some(parent))?;
        // Implicit3DTileContent constructs these directly, outside loadTileset's
        // explicit-tree depth assignment. Preserve that source distinction.
        self.tiles[tile].depth = 0;
        self.tiles[tile].implicit_coordinates = Some(c);
        self.tiles[tile].implicit_subtree = Some(Rc::clone(subtree));
        self.tiles[tile].implicit_metadata = metadata;
        Ok(tile)
    }
    pub fn expand_subtree(
        &mut self,
        placeholder: TileId,
        subtree: Rc<ImplicitSubtree>,
        description: ImplicitTileset,
        coordinates: ImplicitTileCoordinates,
    ) -> Result<TileId, RuntimeError> {
        let root =
            self.implicit_tile(placeholder, &description, &subtree, coordinates, 0, coordinates.child_index())?;
        let mut row = vec![Some(root)];
        for level in 1..description.subtree_levels {
            let offset = subtree.get_level_offset(level) as usize;
            let mut next = Vec::with_capacity(row.len() * description.branching_factor as usize);
            for morton in 0..row.len() * description.branching_factor as usize {
                let index = offset + morton;
                if !subtree.tile_is_available_at_index(index).map_err(runtime)? {
                    next.push(None);
                    continue;
                }
                let parent = row[morton / description.branching_factor as usize]
                    .ok_or_else(|| RuntimeError::new("implicit child is available without its parent"))?;
                let child_index = (morton % description.branching_factor as usize) as u32;
                let c = self.tiles[parent]
                    .implicit_coordinates
                    .unwrap()
                    .get_child_coordinates(child_index as i64)
                    .map_err(runtime)?;
                next.push(Some(self.implicit_tile(parent, &description, &subtree, c, index, child_index)?));
            }
            row = next;
        }
        for (index, parent) in row.into_iter().enumerate() {
            let Some(parent) = parent else { continue };
            for child in 0..description.branching_factor {
                if !subtree
                    .child_subtree_is_available_at_index(index * description.branching_factor as usize + child as usize)
                    .map_err(runtime)?
                {
                    continue;
                }
                let c = self.tiles[parent]
                    .implicit_coordinates
                    .unwrap()
                    .get_child_coordinates(child as i64)
                    .map_err(runtime)?;
                let volume = derive_implicit_volume(
                    &description.bounding_volume,
                    c,
                    Some((&self.tiles[parent].source_bounds, child)),
                )?;
                let error = description.geometric_error.unwrap_or(f64::NAN) / 2f64.powi(c.level as i32);
                let header = json!({"boundingVolume":volume,"geometricError":error,"refine":description.refine,"contents":[{"uri":template(&description.subtree_uri_template,c)?.url()}]});
                let id = self.make_tile(&description.base_resource, &header, Some(parent))?;
                self.tiles[id].depth = 0;
                self.tiles[id].implicit_tileset = Some(description.clone());
                self.tiles[id].implicit_coordinates = Some(c);
            }
        }
        Ok(root)
    }
    pub fn destroy_subtree(&mut self, root: TileId) -> Result<(), RuntimeError> {
        let mut stack = std::mem::take(&mut self.tiles[root].children);
        while let Some(id) = stack.pop() {
            stack.extend(self.tiles[id].children.clone());
            self.unload_tile(id)?;
            let tile = &mut self.tiles[id];
            if let Some(mut content) = tile.content.take() {
                content.destroy()?;
            }
            if let Some(mut content) = tile.expired_content.take() {
                content.destroy()?;
            }
            tile.destroyed = true;
            self.statistics.number_of_tiles_total = self.statistics.number_of_tiles_total.saturating_sub(1);
        }
        let mut i = 0;
        while i < self.pending.len() {
            if self.tiles[self.pending[i].id].destroyed {
                let pending = self.pending.remove(i);
                pending.cancelled.set(true);
                self.statistics.number_of_pending_requests =
                    self.statistics.number_of_pending_requests.saturating_sub(pending.counted);
            } else {
                i += 1;
            }
        }
        Ok(())
    }
}
