//! Cesium `AutoExposure`: gets luminance value at each pixel and uses
//! parallel reduction to compute average scene luminance in a 1x1 texture.
use crate::{
    core::{PixelFormat, RuntimeError},
    renderer::*,
};
use std::rc::Rc;

fn get_auto_exposure_shader(index: usize, length: usize) -> String {
    let mut source = String::from(
        "uniform sampler2D colorTexture;\n\
in vec2 v_textureCoordinates;\n\
float sampleTexture(vec2 offset) {\n",
    );
    if index == 0 {
        source.push_str(
            "    vec4 color = texture(colorTexture, v_textureCoordinates + offset);\n\
    return czm_luminance(color.rgb);\n",
        );
    } else {
        source.push_str("    return texture(colorTexture, v_textureCoordinates + offset).r;\n");
    }
    source.push_str(
        "}\n\n\
uniform vec2 colorTextureDimensions;\n\
uniform vec2 minMaxLuminance;\n\
uniform sampler2D previousLuminance;\n\
void main() {\n\
    float color = 0.0;\n\
    float xStep = 1.0 / colorTextureDimensions.x;\n\
    float yStep = 1.0 / colorTextureDimensions.y;\n\
    int count = 0;\n\
    for (int i = 0; i < 3; ++i) {\n\
        for (int j = 0; j < 3; ++j) {\n\
            vec2 offset;\n\
            offset.x = -xStep + float(i) * xStep;\n\
            offset.y = -yStep + float(j) * yStep;\n\
            if (offset.x < 0.0 || offset.x > 1.0 || offset.y < 0.0 || offset.y > 1.0) {\n\
                continue;\n\
            }\n\
            color += sampleTexture(offset);\n\
            ++count;\n\
        }\n\
    }\n\
    if (count > 0) {\n\
        color /= float(count);\n\
    }\n",
    );

    if index == length - 1 {
        source.push_str(
            "    float previous = texture(previousLuminance, vec2(0.5)).r;\n\
    color = clamp(color, minMaxLuminance.x, minMaxLuminance.y);\n\
    color = previous + (color - previous) / (60.0 * 1.5);\n\
    color = clamp(color, minMaxLuminance.x, minMaxLuminance.y);\n",
        );
    }

    source.push_str("    out_FragColor = vec4(color);\n}\n");
    source
}

pub struct AutoExposure<G: Gl> {
    name: String,
    pub enabled: bool,
    pub minimum_luminance: f64,
    pub maximum_luminance: f64,
    ready: bool,

    framebuffers: Vec<FramebufferManager<G>>,
    previous_luminance: Option<FramebufferManager<G>>,
    commands: Vec<ViewportCommand<G>>,

    width: Option<u32>,
    height: Option<u32>,
    destroyed: bool,
}

impl<G: Gl> Default for AutoExposure<G> {
    fn default() -> Self {
        Self::new()
    }
}

impl<G: Gl> AutoExposure<G> {
    pub fn new() -> Self {
        Self {
            name: "czm_autoexposure".to_string(),
            enabled: true,
            minimum_luminance: 0.1,
            maximum_luminance: 10.0,
            ready: false,
            framebuffers: Vec::new(),
            previous_luminance: None,
            commands: Vec::new(),
            width: None,
            height: None,
            destroyed: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn output_texture(&self) -> Option<Rc<Texture<G>>> {
        self.framebuffers.last().and_then(|fb| fb.color_texture(0).ok().flatten().cloned())
    }

    fn destroy_framebuffers(&mut self) -> Result<(), RuntimeError> {
        for fb in &mut self.framebuffers {
            fb.destroy()?;
        }
        self.framebuffers.clear();
        if let Some(mut prev) = self.previous_luminance.take() {
            prev.destroy()?;
        }
        Ok(())
    }

    fn destroy_commands(&mut self) -> Result<(), RuntimeError> {
        for cmd in &mut self.commands {
            cmd.shader_program.destroy()?;
        }
        self.commands.clear();
        Ok(())
    }

    pub fn update(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        let drawing_size = context.drawing_buffer_size().unwrap_or([1, 1]);
        let width = drawing_size[0];
        let height = drawing_size[1];

        if self.width != Some(width) || self.height != Some(height) {
            self.width = Some(width);
            self.height = Some(height);

            self.destroy_framebuffers()?;
            self.destroy_commands()?;

            let datatype = if context.capabilities()?.half_floating_point_texture {
                PixelDatatype::HalfFloat
            } else {
                PixelDatatype::Float
            };

            let max_dim = width.max(height) as f64;
            let length = (max_dim.ln() / 3.0_f64.ln()).ceil().max(0.0) as usize;

            let mut cur_w = width;
            let mut cur_h = height;
            for _ in 0..length {
                cur_w = ((cur_w as f64) / 3.0).ceil().max(1.0) as u32;
                cur_h = ((cur_h as f64) / 3.0).ceil().max(1.0) as u32;

                let opts = FramebufferManagerOptions {
                    color: true,
                    pixel_datatype: Some(datatype),
                    create_depth_attachments: false,
                    ..Default::default()
                };
                let mut manager = FramebufferManager::new(opts)?;
                manager.update(context, cur_w, cur_h, Some(1), Some(datatype), Some(PixelFormat::Rgba))?;
                self.framebuffers.push(manager);
            }

            let prev_opts = FramebufferManagerOptions {
                color: true,
                pixel_datatype: Some(datatype),
                create_depth_attachments: false,
                ..Default::default()
            };
            let mut prev_manager = FramebufferManager::new(prev_opts)?;
            prev_manager.update(context, cur_w, cur_h, Some(1), Some(datatype), Some(PixelFormat::Rgba))?;
            self.previous_luminance = Some(prev_manager);

            for i in 0..length {
                let source = get_auto_exposure_shader(i, length);
                let cmd = context.create_viewport_quad_command(
                    ShaderSource::new(&source),
                    ViewportCommandOptions {
                        framebuffer: self.framebuffers[i].framebuffer().cloned(),
                        ..Default::default()
                    },
                )?;
                self.commands.push(cmd);
            }

            self.ready = true;
        }

        // Ping-pong previous luminance target
        if let Some(prev) = self.previous_luminance.as_mut() {
            if let Some(last_fb) = self.framebuffers.last_mut() {
                std::mem::swap(last_fb, prev);
                if let Some(last_cmd) = self.commands.last_mut() {
                    last_cmd.options.framebuffer = last_fb.framebuffer().cloned();
                }
            }
        }

        Ok(())
    }

    pub fn execute(&mut self, context: &Context<G>, color_texture: &Rc<Texture<G>>) -> Result<(), RuntimeError> {
        if !self.enabled || !self.ready || self.commands.is_empty() {
            return Ok(());
        }

        let length = self.commands.len();
        let pass = PassState::default();

        for i in 0..length {
            let cmd = &mut self.commands[i];
            cmd.textures.clear();
            cmd.uniforms.clear();

            if i == 0 {
                cmd.textures.push(("colorTexture".into(), TextureBinding::Texture(color_texture.clone())));
                cmd.uniforms.push((
                    "colorTextureDimensions".into(),
                    UniformValue::Float(vec![color_texture.width() as f32, color_texture.height() as f32]),
                ));
            } else {
                let prev_tex = self.framebuffers[i - 1]
                    .color_texture(0)?
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("Missing previous level texture in AutoExposure"))?;
                cmd.textures.push(("colorTexture".into(), TextureBinding::Texture(prev_tex.clone())));
                cmd.uniforms.push((
                    "colorTextureDimensions".into(),
                    UniformValue::Float(vec![prev_tex.width() as f32, prev_tex.height() as f32]),
                ));
            }

            if i == length - 1 {
                if let Some(prev) = &self.previous_luminance {
                    let prev_lum_tex = prev
                        .color_texture(0)?
                        .cloned()
                        .ok_or_else(|| RuntimeError::new("Missing previous luminance texture"))?;
                    cmd.textures.push(("previousLuminance".into(), TextureBinding::Texture(prev_lum_tex)));
                }
                cmd.uniforms.push((
                    "minMaxLuminance".into(),
                    UniformValue::Float(vec![self.minimum_luminance as f32, self.maximum_luminance as f32]),
                ));
            }

            cmd.execute(context, &pass)?;
        }

        Ok(())
    }

    pub fn clear(&self, context: &Context<G>) -> Result<(), RuntimeError> {
        let pass = PassState::default();
        let clear_cmd = ClearCommand {
            color: Some([0.0, 0.0, 0.0, 0.0]),
            ..Default::default()
        };
        for fb in &self.framebuffers {
            fb.clear(context, &clear_cmd, &pass)?;
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroy_framebuffers()?;
        self.destroy_commands()?;
        self.destroyed = true;
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
