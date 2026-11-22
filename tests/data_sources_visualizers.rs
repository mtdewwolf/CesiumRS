use cesium_rs::{
    core::{Cartesian3, JulianDate, ReferenceFrame, RuntimeError},
    data_sources::{
        path_sub_sample, ConstantPositionProperty, Entity, EntityCluster, EntityClusterOptions, PositionPropertyRef,
    },
    renderer::{Gl, GlValue},
};
use std::rc::Rc;

struct Host;
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, _: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        Ok(GlValue::Null)
    }
}

#[test]
fn cluster_reuses_released_primitives() {
    let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
    let first = Entity::with_id("first");
    let second = Entity::with_id("second");
    cluster.get_billboard(&first).unwrap();
    cluster.get_label(&first).unwrap();
    cluster.get_point(&first).unwrap();
    cluster.remove_billboard(&first);
    cluster.remove_label(&first);
    cluster.remove_point(&first);
    cluster.get_billboard(&second).unwrap();
    cluster.get_label(&second).unwrap();
    cluster.get_point(&second).unwrap();
}

#[test]
fn constant_path_has_one_position() {
    let position = Cartesian3::new(1., 2., 3.);
    let property: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(Some(position), ReferenceFrame::Fixed));
    let start = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let points = path_sub_sample(&property, &start, &start.add_seconds(60.), None, ReferenceFrame::Fixed, 10.).unwrap();
    assert_eq!(points, vec![position]);
}
