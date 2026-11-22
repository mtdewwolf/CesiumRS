use cesium_rs::{
    core::{
        BoundingSphere, Cartesian2, Cartesian3, Ellipsoid, GeographicProjection, JulianDate, ReferenceFrame, Resource,
        RuntimeError,
    },
    data_sources::{
        path_sub_sample, CallbackPositionProperty, Cesium3DTilesetGraphics, Cesium3DTilesetVisualizer,
        ConstantPositionProperty, Entity, EntityCluster, EntityClusterOptions, EntityCollection, EntityView,
        PositionPropertyRef, PropertyInput, ScaledPositionProperty, Visualizer,
    },
    renderer::{Gl, GlValue},
    scene::{Camera, CameraOptions, Cesium3DTileset, SceneMode, SceneTransformState, TilesetOptions},
};
use futures_util::FutureExt;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

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
fn generic_path_respects_subminute_resolution() {
    let source: PositionPropertyRef = Rc::new(CallbackPositionProperty::new(
        Rc::new(|_, _| Ok(Some(Cartesian3::new(6378137., 1., 1.)))),
        false,
        ReferenceFrame::Fixed,
    ));
    let property: PositionPropertyRef = Rc::new(ScaledPositionProperty::new(Some(source)));
    let start = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let points = path_sub_sample(&property, &start, &start.add_seconds(30.), None, ReferenceFrame::Fixed, 1.).unwrap();
    assert_eq!(points.len(), 31);
}

#[test]
fn tracked_sphere_updates_and_clears() {
    let entity = Entity::with_id("tracked");
    entity
        .set_position(Some(Rc::new(ConstantPositionProperty::new(
            Some(Cartesian3::new(6378137., 0., 0.)),
            ReferenceFrame::Fixed,
        ))))
        .unwrap();
    let mut view = EntityView::new(entity, None);
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let time = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let first = BoundingSphere::new(Cartesian3::new(6378137., 0., 0.), 10.);
    let second = BoundingSphere::new(Cartesian3::new(6378137., 100., 0.), 10.);
    view.update(&time, SceneMode::Scene3D, &mut camera, Some(first)).unwrap();
    view.update(&time, SceneMode::Scene3D, &mut camera, Some(second)).unwrap();
    assert_eq!(view.bounding_sphere, Some(second));
    view.update(&time, SceneMode::Scene3D, &mut camera, None).unwrap();
    assert_eq!(view.bounding_sphere, None);
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
fn tileset_is_hidden_when_uri_is_removed() {
    let collection = EntityCollection::new();
    let entity = Entity::with_id("tileset");
    let graphics = Rc::new(Cesium3DTilesetGraphics::new());
    graphics.set_uri(Some(PropertyInput::value("https://example.com/tileset.json"))).unwrap();
    entity.set_property("tileset", Some(PropertyInput::Component(graphics.clone()))).unwrap();
    collection.add(entity).unwrap();
    let factory = Rc::new(|resource: Resource| {
        async move {
            let tileset = Cesium3DTileset::from_json(
                resource,
                &json!({
                    "asset": {"version":"1.1"}, "geometricError": 100,
                    "root": {"boundingVolume":{"sphere":[6378137,0,0,10]}, "geometricError":0}
                }),
                TilesetOptions::default(),
                None,
            )?;
            Ok(Rc::new(RefCell::new(tileset)))
        }
        .boxed_local()
    });
    let mut visualizer = Cesium3DTilesetVisualizer::new(collection, factory);
    let time = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    visualizer.update(&time).unwrap();
    let tileset = visualizer.tileset_for("tileset").unwrap();
    assert!(tileset.borrow().options.show);
    graphics.set_uri(None).unwrap();
    visualizer.update(&time).unwrap();
    assert!(!tileset.borrow().options.show);
}

#[test]
fn disabling_cluster_categories_restores_source_visibility() {
    let cluster = EntityCluster::<Host>::new(EntityClusterOptions { enabled: true, ..Default::default() });
    let entity = Entity::with_id("point");
    let point = cluster.get_point(&entity).unwrap();
    point.set_cluster_show(false);
    cluster.set_cluster_points(false);
    let projection = GeographicProjection::default();
    let state = SceneTransformState {
        mode: SceneMode::Scene3D,
        morph_time: 1.,
        map_projection: &projection,
        client_dimensions: Cartesian2::new(800., 600.),
        drawing_buffer_dimensions: Cartesian2::new(800., 600.),
    };
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
    assert!(point.cluster_show());
    point.set_cluster_show(false);
    cluster.set_cluster_labels(false);
    cluster.set_cluster_billboards(false);
    cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
    assert!(point.cluster_show());
}

#[test]
fn constant_path_has_one_position() {
    let position = Cartesian3::new(1., 2., 3.);
    let property: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(Some(position), ReferenceFrame::Fixed));
    let start = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let points = path_sub_sample(&property, &start, &start.add_seconds(60.), None, ReferenceFrame::Fixed, 10.).unwrap();
    assert_eq!(points, vec![position]);
}
