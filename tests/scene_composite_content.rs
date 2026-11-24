use cesium_rs::{
    core::{Cartesian3, Color, JulianDate, Ray, Resource, RuntimeError, TimeStandard},
    renderer::UniformFrame,
    scene::{Camera, CameraOptions, Composite3DTileContent, FrameState, TileContentMetrics, TilesetContent},
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Child {
    id: usize,
    ready: Rc<Cell<bool>>,
    calls: Rc<RefCell<Vec<String>>>,
}
impl TilesetContent for Child {
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics { features: 1, ..Default::default() }
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(self.ready.get())
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.calls.borrow_mut().push(format!("update{}", self.id));
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(Some(Cartesian3::new(if self.id == 0 { 4. } else { 2. }, 0., 0.)))
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.calls.borrow_mut().push(format!("destroy{}", self.id));
        Ok(())
    }
}
fn frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(32., 32.));
    let uniform = camera.uniform_camera();
    let volume = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    FrameState::new(UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)), volume, [32, 32])
}
#[test]
fn readiness_pick_and_ownership_match_cesium() {
    let fixture: Value = serde_json::from_str(include_str!("data/scene-composite-content.json")).unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let flags = [Rc::new(Cell::new(true)), Rc::new(Cell::new(false))];
    let children = flags
        .iter()
        .enumerate()
        .map(|(id, ready)| {
            Box::new(Child { id, ready: ready.clone(), calls: calls.clone() }) as Box<dyn TilesetContent>
        })
        .collect();
    let mut content =
        Composite3DTileContent::new(Resource::new("https://example.com/tile.cmpt?x=1").unwrap(), children);
    let mut frame = frame();
    assert_eq!(content.url(), fixture["url"]);
    assert_eq!(content.ready(), fixture["initialReady"]);
    let ray = Ray::new(Cartesian3::ZERO, Cartesian3::UNIT_X);
    assert_eq!(content.pick(&ray, &frame).unwrap(), None);
    content.update(&mut frame).unwrap();
    assert_eq!(content.ready(), fixture["firstReady"]);
    flags[1].set(true);
    content.update(&mut frame).unwrap();
    assert_eq!(content.ready(), fixture["secondReady"]);
    assert_eq!(content.pick(&ray, &frame).unwrap().unwrap().x, fixture["pick"].as_f64().unwrap());
    flags[0].set(false);
    content.update(&mut frame).unwrap();
    assert_eq!(content.ready(), fixture["stickyReady"]);
    content.apply_debug_settings(true, Color::WHITE).unwrap();
    content.destroy().unwrap();
    assert_eq!(content.is_destroyed(), fixture["destroyed"]);
    assert_eq!(serde_json::to_value(&*calls.borrow()).unwrap(), fixture["calls"]);
    assert!(content.update(&mut frame).is_err());
}

#[test]
fn child_offsets_and_resource_indices_match_cesium() {
    use futures_util::FutureExt;
    let fixture: Value = serde_json::from_str(include_str!("data/scene-composite-content.json")).unwrap();
    let mut bytes = vec![0u8; 40];
    bytes[..4].copy_from_slice(b"cmpt");
    for (offset, n) in [(4, 1u32), (8, 40), (12, 2), (24, 12), (36, 12)] {
        bytes[offset..offset + 4].copy_from_slice(&n.to_le_bytes());
    }
    bytes[16..20].copy_from_slice(b"b3dm");
    bytes[28..32].copy_from_slice(b"vctr");
    let resource = Resource::new("https://example.com/tile.cmpt?compositeIndex=3&x=1").unwrap();
    let mut found = Vec::new();
    let result = Composite3DTileContent::from_tile_type(resource, &bytes, 0, |resource, child| {
        let magic = std::str::from_utf8(&child[..4]).unwrap();
        let offset = child.as_ptr() as usize - bytes.as_ptr() as usize;
        found.push(
            serde_json::json!({"magic": magic, "url": resource.get_url_component(true, false), "offset": offset}),
        );
        Box::pin(async { Ok(Box::new(cesium_rs::scene::Empty3DTileContent::new()) as Box<dyn TilesetContent>) })
    })
    .now_or_never()
    .unwrap();
    assert!(result.is_ok());
    assert_eq!(serde_json::to_value(found).unwrap(), fixture["children"]);
    bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
    let result = Composite3DTileContent::from_tile_type(
        Resource::new("https://example.com/tile.cmpt").unwrap(),
        &bytes,
        0,
        |_, _| unreachable!(),
    )
    .now_or_never()
    .unwrap();
    assert_eq!(result.err().unwrap().message, fixture["versionError"].as_str().unwrap());
}
