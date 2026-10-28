//! Scene viewport material quad, backed by executable renderer commands.
//! Hosts resolve texture uniforms through the returned command's texture bindings.
use super::material::Material;
use crate::{core::RuntimeError, renderer::*};
use std::{cell::RefCell, rc::Rc};
type Result<T> = std::result::Result<T, RuntimeError>;
pub struct ViewportQuad<G: Gl> {
    pub show: bool,
    pub rectangle: Viewport,
    pub material: Rc<RefCell<Material>>,
    previous_material: Option<Rc<RefCell<Material>>>,
    command: Option<ViewportCommand<G>>,
    destroyed: bool,
}
impl<G: Gl> ViewportQuad<G> {
    pub fn new(rectangle: Viewport, material: Rc<RefCell<Material>>) -> Self {
        Self { show: true, rectangle, material, previous_material: None, command: None, destroyed: false }
    }
    pub fn update(&mut self, context: &Context<G>, render: bool) -> Result<Option<&mut ViewportCommand<G>>> {
        if self.destroyed {
            return Err(RuntimeError::new("ViewportQuad was destroyed"));
        }
        if !self.show || !render {
            return Ok(None);
        }
        if self.command.is_none() || !self.previous_material.as_ref().is_some_and(|p| Rc::ptr_eq(p, &self.material)) {
            let material = self.material.borrow();
            let shader = ShaderSource {
                sources: vec![material.shader_source.clone(), crate::shaders::root::VIEWPORT_QUAD_FS.source.into()],
                ..Default::default()
            };
            let new_command = context.create_viewport_quad_command(
                shader,
                ViewportCommandOptions { pass: Some(Pass::Overlay), ..Default::default() },
            )?;
            super::framebuffer_helpers::destroy_command(&mut self.command)?;
            self.command = Some(new_command);
            self.previous_material = Some(self.material.clone());
        }
        let command = self.command.as_mut().unwrap();
        command.options.render_state = RenderState {
            viewport: Some(self.rectangle),
            blending: super::blending_state::alpha_blend(),
            ..Default::default()
        };
        command.uniforms = self
            .material
            .borrow()
            .uniform_bindings()
            .into_iter()
            .filter_map(|(name, value)| {
                use super::material::MaterialValue;
                let numbers = || value.numbers().unwrap_or_default().into_iter().map(|v| v as f32).collect();
                let uniform = match &value {
                    MaterialValue::Bool(v) => UniformValue::Int(vec![i32::from(*v)]),
                    MaterialValue::IntVec3(v) => UniformValue::Int(vec![v.x as i32, v.y as i32, v.z as i32]),
                    MaterialValue::Mat2(_) => UniformValue::Matrix { dimension: 2, values: numbers() },
                    MaterialValue::Mat3(_) => UniformValue::Matrix { dimension: 3, values: numbers() },
                    MaterialValue::Mat4(_) => UniformValue::Matrix { dimension: 4, values: numbers() },
                    MaterialValue::Channels(_) | MaterialValue::Sampler2D(_) | MaterialValue::SamplerCube(_) => {
                        return None
                    }
                    _ => UniformValue::Float(numbers()),
                };
                Some((name, uniform))
            })
            .collect();
        Ok(Some(command))
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        super::framebuffer_helpers::destroy_command(&mut self.command)?;
        self.previous_material = None;
        self.destroyed = true;
        Ok(())
    }
}
