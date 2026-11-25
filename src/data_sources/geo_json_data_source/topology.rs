//! Expand shared TopoJSON arcs into GeoJSON feature geometries.
use super::{array, coordinate};
use crate::core::RuntimeError;
use serde_json::{json, Value};

pub(super) struct Topology<'a> {
    arcs: &'a Value,
    transform: Option<([f64; 3], [f64; 3])>,
}
impl<'a> Topology<'a> {
    pub fn new(topology: &'a Value) -> Result<Self, RuntimeError> {
        let transform = &topology["transform"];
        let transform = if transform.is_null() {
            None
        } else {
            Some((coordinate(&transform["scale"])?.0, coordinate(&transform["translate"])?.0))
        };
        Ok(Self { arcs: &topology["arcs"], transform })
    }
    fn point(&self, p: &Value, sum: &mut [f64; 2]) -> Result<Value, RuntimeError> {
        let (mut p, len) = coordinate(p)?;
        if let Some((scale, translate)) = &self.transform {
            for axis in 0..2 {
                sum[axis] += p[axis];
                p[axis] = sum[axis] * scale[axis] + translate[axis];
            }
        }
        Ok(json!(&p[..len]))
    }
    fn line(&self, indices: &Value, min_len: usize) -> Result<Value, RuntimeError> {
        let mut line = Vec::new();
        for index in array(indices)? {
            let index = index.as_i64().ok_or_else(|| RuntimeError::new("Expected a TopoJSON arc index."))?;
            let absolute = if index < 0 { !index } else { index };
            let arc = self
                .arcs
                .get(absolute as usize)
                .ok_or_else(|| RuntimeError::new(format!("Missing TopoJSON arc {absolute}.")))?;
            line.pop();
            let start = line.len();
            let mut sum = [0.; 2];
            for p in array(arc)? {
                line.push(self.point(p, &mut sum)?);
            }
            if index < 0 {
                line[start..].reverse();
            }
        }
        let first = line.first().ok_or_else(|| RuntimeError::new("Empty TopoJSON line."))?.clone();
        if line.len() < min_len {
            line.resize(min_len, first);
        }
        Ok(Value::Array(line))
    }
    fn polygon(&self, arcs: &Value) -> Result<Value, RuntimeError> {
        array(arcs)?.iter().map(|a| self.line(a, 4)).collect()
    }
    fn geometry(&self, o: &Value) -> Result<Value, RuntimeError> {
        let arcs = &o["arcs"];
        let c = &o["coordinates"];
        let coords = match o["type"].as_str() {
            Some("GeometryCollection") => {
                let children: Vec<_> =
                    array(&o["geometries"])?.iter().map(|g| self.geometry(g)).collect::<Result<_, _>>()?;
                return Ok(json!({"type":"GeometryCollection","geometries":children}));
            }
            Some("Point") => self.point(c, &mut [0.; 2])?,
            Some("MultiPoint") => {
                array(c)?.iter().map(|p| self.point(p, &mut [0.; 2])).collect::<Result<Value, _>>()?
            }
            Some("LineString") => self.line(arcs, 2)?,
            Some("MultiLineString") => array(arcs)?.iter().map(|a| self.line(a, 2)).collect::<Result<Value, _>>()?,
            Some("Polygon") => self.polygon(arcs)?,
            Some("MultiPolygon") => array(arcs)?.iter().map(|a| self.polygon(a)).collect::<Result<Value, _>>()?,
            _ => return Ok(Value::Null),
        };
        Ok(json!({"type":o["type"],"coordinates":coords}))
    }
    pub fn feature(&self, o: &Value) -> Result<Value, RuntimeError> {
        let properties = o.get("properties").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| json!({}));
        Ok(json!({"type":"Feature","id":o["id"],"properties":properties,"geometry":self.geometry(o)?}))
    }
}
