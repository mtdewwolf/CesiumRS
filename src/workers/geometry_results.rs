//! The `PrimitivePipeline.pack/unpackCreateGeometryResults` wire format used by
//! `Workers/createGeometry`. One owned f64 buffer replaces the JS transfer list.

use crate::core::*;

#[derive(Clone, Debug, PartialEq)]
pub struct PackedGeometryResults {
    pub string_table: Vec<String>,
    pub packed_data: Vec<f64>,
}

impl PackedGeometryResults {
    pub fn pack(items: &[Option<Geometry>]) -> Self {
        let mut string_table = Vec::new();
        let mut data = vec![items.len() as f64];
        for item in items {
            data.push(if item.is_some() { 1.0 } else { 0.0 });
            let Some(g) = item else { continue };
            data.extend([
                g.primitive_type as u32 as f64,
                g.geometry_type as u32 as f64,
                GeometryOffsetAttribute::pack(g.offset_attribute),
            ]);
            for sphere in [g.bounding_sphere, g.bounding_sphere_cv] {
                data.push(if sphere.is_some() { 1.0 } else { 0.0 });
                let b = sphere.unwrap_or_default();
                data.extend([b.center.x, b.center.y, b.center.z, b.radius]);
            }
            data.push(g.attributes.iter().count() as f64);
            for (name, attribute) in g.attributes.iter() {
                let index = match string_table.iter().position(|s| s == name) {
                    Some(index) => index,
                    None => {
                        string_table.push(name.to_owned());
                        string_table.len() - 1
                    }
                };
                data.extend([
                    index as f64,
                    attribute.component_datatype as u32 as f64,
                    f64::from(attribute.components_per_attribute),
                    if attribute.normalize { 1.0 } else { 0.0 },
                    attribute.values.len() as f64,
                ]);
                data.extend(attribute.values.to_f64_vec());
            }
            data.push(g.indices.as_ref().map_or(0, TypedArray::len) as f64);
            if let Some(indices) = &g.indices {
                data.extend(indices.to_f64_vec());
            }
        }
        Self { string_table, packed_data: data }
    }

    /// Reads a worker result, validating lengths and enums before allocating.
    pub fn unpack(&self) -> Result<Vec<Option<Geometry>>, RuntimeError> {
        let mut reader = Reader { values: &self.packed_data };
        let count = reader.integer()?;
        if count > reader.values.len() {
            return Err(invalid());
        }
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            if reader.number()? != 1.0 {
                result.push(None);
                continue;
            }
            let primitive_type =
                PrimitiveType::from_gl(reader.integer()?.try_into().map_err(|_| invalid())?).ok_or_else(invalid)?;
            let geometry_type = match reader.integer()? {
                0 => GeometryType::None,
                1 => GeometryType::Triangles,
                2 => GeometryType::Lines,
                3 => GeometryType::Polylines,
                _ => return Err(invalid()),
            };
            let offset = reader.number()?;
            if ![-1.0, 0.0, 1.0, 2.0].contains(&offset) {
                return Err(invalid());
            }
            let offset_attribute = GeometryOffsetAttribute::unpack(offset);
            let bounding_sphere = reader.sphere()?;
            let bounding_sphere_cv = reader.sphere()?;
            let mut attributes = GeometryAttributes::default();
            let mut last_vertex_count = 0;
            let attribute_count = reader.integer()?;
            if attribute_count > reader.values.len() / 5 {
                return Err(invalid());
            }
            for _ in 0..attribute_count {
                let name = self.string_table.get(reader.integer()?).ok_or_else(invalid)?;
                let datatype = ComponentDatatype::from_gl(reader.integer()?.try_into().map_err(|_| invalid())?)
                    .ok_or_else(invalid)?;
                let components = reader.integer()?;
                if !(1..=4).contains(&components) {
                    return Err(invalid());
                }
                let normalize = reader.number()? != 0.0;
                let length = reader.integer()?;
                let values = TypedArray::from_values(datatype, reader.take(length)?);
                last_vertex_count = length / components;
                attributes.set(
                    name,
                    GeometryAttribute {
                        component_datatype: datatype,
                        components_per_attribute: components as u8,
                        normalize,
                        values,
                    },
                );
            }
            let length = reader.integer()?;
            let indices = if length == 0 {
                None
            } else {
                let datatype = if last_vertex_count >= 65536 {
                    ComponentDatatype::UnsignedInt
                } else {
                    ComponentDatatype::UnsignedShort
                };
                Some(TypedArray::from_values(datatype, reader.take(length)?))
            };
            result.push(Some(Geometry {
                attributes,
                indices,
                primitive_type,
                geometry_type,
                bounding_sphere,
                bounding_sphere_cv,
                offset_attribute,
            }));
        }
        if !reader.values.is_empty() {
            return Err(invalid());
        }
        Ok(result)
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::new("Invalid packed geometry results")
}
struct Reader<'a> {
    values: &'a [f64],
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [f64], RuntimeError> {
        let slice = self.values.get(..count).ok_or_else(invalid)?;
        self.values = &self.values[count..];
        Ok(slice)
    }
    fn number(&mut self) -> Result<f64, RuntimeError> {
        Ok(self.take(1)?[0])
    }
    fn integer(&mut self) -> Result<usize, RuntimeError> {
        let value = self.number()?;
        if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value >= usize::MAX as f64 {
            return Err(invalid());
        }
        Ok(value as usize)
    }
    fn sphere(&mut self) -> Result<Option<BoundingSphere>, RuntimeError> {
        let valid = self.number()? == 1.0;
        let v = self.take(4)?;
        Ok(valid.then(|| BoundingSphere::new(Cartesian3::new(v[0], v[1], v[2]), v[3])))
    }
}
