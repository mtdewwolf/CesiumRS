//! CesiumJS `PickFramebuffer`: reusable color/depth target and spiral hit lookup.
//! Object identity is `Rc` identity, matching JavaScript's Set of picked objects.
use crate::{
    core::{BoundingRectangle, Color, RuntimeError},
    renderer::*,
};
use std::{any::Any, collections::HashSet, future::Future, rc::Rc};

type Result<T> = std::result::Result<T, RuntimeError>;
pub type PickedObject = Rc<dyn Any>;

pub struct PickFramebuffer<G: Gl> {
    context: Context<G>,
    framebuffer: FramebufferManager<G>,
    pass_state: PassState<G>,
    dimensions: [u32; 2],
    destroyed: bool,
}
impl<G: Gl> PickFramebuffer<G> {
    pub fn new(context: Context<G>) -> Result<Self> {
        Ok(Self {
            context,
            framebuffer: FramebufferManager::new(FramebufferManagerOptions {
                depth_stencil: true,
                ..Default::default()
            })?,
            pass_state: PassState::default(),
            dimensions: [0; 2],
            destroyed: false,
        })
    }
    pub fn begin(&mut self, rectangle: &BoundingRectangle, viewport: Viewport) -> Result<PassState<G>> {
        self.alive()?;
        self.framebuffer.update(&self.context, viewport.width, viewport.height, None, None, None)?;
        self.dimensions = [viewport.width, viewport.height];
        self.pass_state = PassState {
            framebuffer: self.framebuffer.framebuffer().cloned(),
            viewport: Some(Viewport { x: 0, y: 0, ..viewport }),
            blending_enabled: Some(false),
            scissor_test: Some(ScissorTest {
                enabled: true,
                rectangle: Viewport {
                    x: rectangle.x as i32,
                    y: rectangle.y as i32,
                    width: rectangle.width as u32,
                    height: rectangle.height as u32,
                },
            }),
        };
        Ok(self.pass_state.clone())
    }
    fn alive(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("PickFramebuffer is destroyed"))
        } else {
            Ok(())
        }
    }
    fn read_layout(&self, rectangle: &BoundingRectangle) -> Result<PickLayout> {
        self.alive()?;
        if self.framebuffer.framebuffer().is_none() {
            return Err(RuntimeError::new("PickFramebuffer.begin must be called before readback"));
        }
        PickLayout::new(rectangle, self.dimensions)
    }
    fn pixels(&self, layout: &PickLayout) -> Result<Vec<u8>> {
        let mut pixels = vec![0; layout.length()?];
        if let Some(region) = layout.region {
            let read = self
                .context
                .read_pixels_from(ReadState { region, framebuffer: self.framebuffer.framebuffer().map(Rc::as_ref) })?;
            layout.copy_rows(&read.bytes, &mut pixels);
        }
        Ok(pixels)
    }
    pub fn end(&self, rectangle: &BoundingRectangle, limit: usize) -> Result<Vec<PickedObject>> {
        let layout = self.read_layout(rectangle)?;
        Ok(pick_objects_from_pixels(&self.context, &self.pixels(&layout)?, layout.width, layout.height, limit))
    }
    pub fn read_center_pixel(&self, rectangle: &BoundingRectangle) -> Result<[u8; 4]> {
        let layout = self.read_layout(rectangle)?;
        let pixels = self.pixels(&layout)?;
        let index = 4 * ((layout.height / 2 * layout.width + layout.width / 2) as usize);
        Ok(pixels[index..index + 4].try_into().expect("center of RGBA buffer"))
    }
    /// Issues the transfer and fence immediately. Awaiting the returned request
    /// only polls the fence; the framebuffer can be reused for another pick.
    /// WebGL1 captures the objects synchronously, matching Cesium's fallback.
    pub fn end_async(&self, rectangle: &BoundingRectangle, limit: usize) -> Result<PickReadback<G>> {
        let layout = self.read_layout(rectangle)?;
        let (buffer, sync, objects) = if self.context.webgl2() {
            if let Some(region) = layout.region {
                let buffer = self.context.read_pixels_to_pbo(ReadState {
                    region,
                    framebuffer: self.framebuffer.framebuffer().map(Rc::as_ref),
                })?;
                let sync = Sync::new(&self.context)?;
                (Some(buffer), Some(sync), None)
            } else {
                (None, None, Some(Vec::new()))
            }
        } else {
            (None, None, Some(self.end(rectangle, limit)?))
        };
        Ok(PickReadback { context: self.context.clone(), layout, limit, buffer, sync, objects })
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.framebuffer.destroy()?;
            self.pass_state = PassState::default();
            self.destroyed = true;
        }
        Ok(())
    }
}

/// Host scheduling replaces the source's Scene.afterRender promise callback.
/// Resources are released if waiting fails or the request is dropped.
pub struct PickReadback<G: Gl> {
    context: Context<G>,
    layout: PickLayout,
    limit: usize,
    buffer: Option<Buffer<G>>,
    sync: Option<Sync<G>>,
    objects: Option<Vec<PickedObject>>,
}
impl<G: Gl> PickReadback<G> {
    pub async fn wait<S, F>(mut self, schedule: S) -> Result<Vec<PickedObject>>
    where
        S: FnMut() -> F,
        F: Future<Output = Result<()>>,
    {
        if let Some(objects) = self.objects.take() {
            return Ok(objects);
        }
        if let Some(sync) = &self.sync {
            sync.wait_for_signal(schedule, None)
                .await
                .map_err(|_| RuntimeError::new("Async Picking Request Timeout"))?;
        }
        let mut pixels = vec![0; self.layout.length()?];
        let buffer = self.buffer.as_ref().expect("PBO for asynchronous readback");
        let bytes = buffer
            .get_buffer_data(0, buffer.size_in_bytes())
            .map_err(|_| RuntimeError::new("Async Picking Request Timeout"))?;
        self.layout.copy_rows(&bytes, &mut pixels);
        Ok(pick_objects_from_pixels(&self.context, &pixels, self.layout.width, self.layout.height, self.limit))
    }
}

#[derive(Clone, Copy)]
struct PickLayout {
    width: u32,
    height: u32,
    region: Option<TextureRegion>,
}
impl PickLayout {
    fn new(rectangle: &BoundingRectangle, dimensions: [u32; 2]) -> Result<Self> {
        let width = rectangle.width as u32;
        let height = rectangle.height as u32;
        if width == 0 || height == 0 || !rectangle.x.is_finite() || !rectangle.y.is_finite() {
            return Err(RuntimeError::new("Pick rectangle must have positive dimensions and finite coordinates"));
        }
        // Context.readPixels clamps the origin to zero. WebGL fills reads
        // beyond the upper/right framebuffer boundary with zero; pad those rows
        // explicitly because the portable renderer validates read bounds.
        let x = rectangle.x.max(0.) as u32;
        let y = rectangle.y.max(0.) as u32;
        let rw = width.min(dimensions[0].saturating_sub(x));
        let rh = height.min(dimensions[1].saturating_sub(y));
        let result =
            Self { width, height, region: (rw > 0 && rh > 0).then_some(TextureRegion { x, y, width: rw, height: rh }) };
        result.length()?;
        Ok(result)
    }
    fn length(self) -> Result<usize> {
        (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| RuntimeError::new("Pick readback overflow"))
    }
    fn copy_rows(self, bytes: &[u8], pixels: &mut [u8]) {
        if let Some(r) = self.region {
            let row = r.width as usize * 4;
            let stride = self.width as usize * 4;
            for y in 0..r.height as usize {
                pixels[y * stride..y * stride + row].copy_from_slice(&bytes[y * row..(y + 1) * row]);
            }
        }
    }
}

/// Center-out square spiral, including Cesium's ordering for rectangular and
/// even-sized regions. Missing pixels are transparent and cannot identify hits.
pub fn pick_objects_from_pixels<G: Gl>(
    context: &Context<G>,
    pixels: &[u8],
    width: u32,
    height: u32,
    limit: usize,
) -> Vec<PickedObject> {
    let max = i64::from(width.max(height));
    let half_width = i64::from(width / 2);
    let half_height = i64::from(height / 2);
    let (mut x, mut y, mut dx, mut dy) = (0i64, 0i64, 0i64, -1i64);
    let mut seen = HashSet::new();
    let mut objects = Vec::new();
    for _ in 0..max * max {
        if -half_width <= x && x <= half_width && -half_height <= y && y <= half_height {
            let index = 4 * ((half_height - y) * i64::from(width) + x + half_width);
            if let Some(p) = usize::try_from(index).ok().and_then(|i| pixels.get(i..i + 4)) {
                let color = Color::bytes_to_rgba(i32::from(p[0]), i32::from(p[1]), i32::from(p[2]), i32::from(p[3]));
                if let Some(object) = context.get_object_by_pick_color(color) {
                    if seen.insert(Rc::as_ptr(&object) as *const ()) {
                        objects.push(object);
                    }
                    if objects.len() >= limit {
                        break;
                    }
                }
            }
        }
        if x == y || (x < 0 && -x == y) || (x > 0 && x == 1 - y) {
            (dx, dy) = (-dy, dx);
        }
        x += dx;
        y += dy;
    }
    objects
}
