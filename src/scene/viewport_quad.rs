//! Scene viewport material quad, backed by executable renderer commands.
use super::{
    frame_state::FrameState,
    material::Material,
    scene::{SceneCommand, ScenePrimitive},
    Cesium3DTilePass,
};
use crate::{core::RuntimeError, renderer::*};
use std::{cell::RefCell, rc::Rc};
type Result<T> = std::result::Result<T, RuntimeError>;
pub struct ViewportQuad<G: Gl> {
    pub show: bool,
    pub rectangle: Viewport,
    pub material: Rc<RefCell<Material>>,
    previous_material: Option<Rc<RefCell<Material>>>,
    command: Option<ViewportCommand<G>>,
    scene_command: Option<Rc<RefCell<ViewportCommand<G>>>>,
    destroyed: bool,
}
impl<G: Gl + 'static> ViewportQuad<G> {
    pub fn new(rectangle: Viewport, material: Rc<RefCell<Material>>) -> Self {
        Self {
            show: true,
            rectangle,
            material,
            previous_material: None,
            command: None,
            scene_command: None,
            destroyed: false,
        }
    }
    fn build_viewport_command(&self, context: &Context<G>) -> Result<ViewportCommand<G>> {
        let material = self.material.borrow();
        let shader = ShaderSource {
            sources: vec![material.shader_source.clone(), crate::shaders::root::VIEWPORT_QUAD_FS.source.into()],
            ..Default::default()
        };
        context.create_viewport_quad_command(
            shader,
            ViewportCommandOptions { pass: Some(Pass::Overlay), ..Default::default() },
        )
    }
    fn sync_viewport_command(&self, context: &Context<G>, command: &mut ViewportCommand<G>) -> Result<()> {
        command.options.render_state = RenderState {
            viewport: Some(self.rectangle),
            blending: super::blending_state::alpha_blend(),
            ..Default::default()
        };
        let material = self.material.borrow();
        command.uniforms = material.uniform_values();
        command.textures = material.resolve_texture_bindings(context)?;
        Ok(())
    }
    pub fn update(&mut self, context: &Context<G>, render: bool) -> Result<Option<&mut ViewportCommand<G>>> {
        if self.destroyed {
            return Err(RuntimeError::new("ViewportQuad was destroyed"));
        }
        if !self.show || !render {
            return Ok(None);
        }
        if self.command.is_none() || !self.previous_material.as_ref().is_some_and(|p| Rc::ptr_eq(p, &self.material)) {
            let new_command = self.build_viewport_command(context)?;
            self.command = Some(new_command);
            self.scene_command = None;
            self.previous_material = Some(self.material.clone());
        }
        let mut cmd = self.command.take().unwrap();
        let sync_res = self.sync_viewport_command(context, &mut cmd);
        self.command = Some(cmd);
        sync_res?;
        Ok(self.command.as_mut())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        self.command = None;
        self.scene_command = None;
        self.previous_material = None;
        self.destroyed = true;
        Ok(())
    }
}

impl<G: Gl + 'static> ScenePrimitive for ViewportQuad<G> {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("ViewportQuad was destroyed"));
        }
        if !self.show || !frame.passes.render {
            return Ok(());
        }
        let Some(context) = frame.context.as_ref().and_then(|c| c.get::<Context<G>>()).cloned() else {
            return Ok(());
        };
        if self.scene_command.is_none()
            || !self.previous_material.as_ref().is_some_and(|p| Rc::ptr_eq(p, &self.material))
        {
            let new_cmd = self.build_viewport_command(&context)?;
            self.scene_command = Some(Rc::new(RefCell::new(new_cmd)));
            self.previous_material = Some(self.material.clone());
        }
        let cmd_rc = self.scene_command.as_ref().unwrap().clone();
        self.sync_viewport_command(&context, &mut cmd_rc.borrow_mut())?;
        let execute = Rc::new(move |submission: super::SceneSubmission| {
            let framebuffer = submission
                .framebuffer
                .as_ref()
                .and_then(|f| f.get::<Rc<Framebuffer<G>>>())
                .map(Rc::clone);
            let pass = PassState {
                framebuffer,
                viewport: submission.viewport,
                scissor_test: submission.scissor_test,
                blending_enabled: submission.blending_enabled,
            };
            cmd_rc.borrow().execute(&context, &pass)
        });
        frame.command_list.push(SceneCommand::new("ViewportQuad", Pass::Overlay, execute));
        Ok(())
    }
    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, _frame: &mut FrameState, _pass: Cesium3DTilePass) -> Result<()> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<()> {
        ViewportQuad::destroy(self)
    }
}
