#[cfg(target_arch = "wasm32")]
mod browser {
    use cesium_rs::{
        renderer::{browser::BrowserGl, *},
        scene::{
            brdf_lut_generator::BrdfLutGenerator, post_process_stage_collection::PostProcessStageCollection,
            post_process_stage_composite::*, post_process_stage_library::PostProcessStageLibrary,
        },
    };
    use std::rc::Rc;
    use wasm_bindgen::prelude::*;
    fn error(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }
    #[wasm_bindgen]
    pub fn brdf(gl: JsValue) -> Result<Vec<u8>, JsValue> {
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let mut generator = BrdfLutGenerator::new();
        generator.update(&context).map_err(error)?;
        let texture = generator.color_texture().unwrap().clone();
        let fb = Rc::new(
            Framebuffer::new(
                &context,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(texture)],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?,
        );
        let bytes = context
            .read_pixels_from(ReadState {
                region: TextureRegion { x: 0, y: 0, width: 256, height: 256 },
                framebuffer: Some(&fb),
            })
            .map_err(error)?;
        generator.update(&context).map_err(error)?;
        generator.destroy();
        fb.destroy().map_err(error)?;
        Ok(bytes.bytes)
    }
    #[wasm_bindgen]
    pub fn exposure(gl: JsValue) -> Result<Vec<f32>, JsValue> {
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let color = Rc::new(
            Texture::new(&context, TextureOptions::rgba(8, 8), Some(&[128, 64, 32, 255].repeat(64))).map_err(error)?,
        );
        let mut exposure = cesium_rs::scene::auto_exposure::AutoExposure::new();
        let mut result = Vec::new();
        for _ in 0..3 {
            exposure.update(&context).map_err(error)?;
            exposure.clear(&context).map_err(error)?;
            exposure.execute(&context, &color).map_err(error)?;
            let fb = Framebuffer::new(
                &context,
                FramebufferOptions {
                    colors: vec![Attachment::Texture(exposure.output_texture().unwrap())],
                    destroy_attachments: false,
                    ..Default::default()
                },
            )
            .map_err(error)?;
            let pixels = context
                .read_pixels_from(ReadState {
                    region: TextureRegion { x: 0, y: 0, width: 1, height: 1 },
                    framebuffer: Some(&fb),
                })
                .map_err(error)?;
            result.extend(pixels.bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())));
            fb.destroy().map_err(error)?;
        }
        exposure.destroy().map_err(error)?;
        color.destroy().map_err(error)?;
        Ok(result)
    }
    #[wasm_bindgen]
    pub fn post_process(gl: JsValue, kind: &str) -> Result<Vec<u8>, JsValue> {
        let context = Context::new_webgl2(BrowserGl::new(gl).map_err(error)?);
        let bytes: Vec<u8> = (0..64).flat_map(|i| [(i * 3) as u8, (255 - i * 2) as u8, (i * 4) as u8, 255]).collect();
        let color = Rc::new(Texture::new(&context, TextureOptions::rgba(8, 8), Some(&bytes)).map_err(error)?);
        let mut collection = PostProcessStageCollection::new(&context).map_err(error)?;
        collection.fxaa_mut().set_enabled(false);
        let stage = PostProcessStageLibrary::create_black_and_white_stage().map_err(error)?;
        if kind == "nested" {
            let mut item = PostProcessStageItem::Stage(stage);
            for _ in 0..4 {
                item = PostProcessStageComposite::new(PostProcessStageCompositeOptions::new(vec![item]))
                    .map_err(error)?
                    .into();
            }
            collection.add(item).map_err(error)?;
        } else {
            collection.add(stage).map_err(error)?;
        }
        collection.update(&context, false, false).map_err(error)?;
        collection.clear(&context).map_err(error)?;
        collection.execute(&context, &color, None, None).map_err(error)?;
        collection.copy(&context, None, &PassState::default()).map_err(error)?;
        let result = context.read_pixels(TextureRegion { x: 0, y: 0, width: 8, height: 8 }).map_err(error)?;
        collection.destroy().map_err(error)?;
        color.destroy().map_err(error)?;
        Ok(result)
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
