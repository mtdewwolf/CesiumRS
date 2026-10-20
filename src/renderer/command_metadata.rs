//! Scene-facing DrawCommand properties, flags and dirty-state tracking.
use super::*;
use crate::core::{BoundingSphere, Matrix4, OrientedBoundingBox};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};
#[derive(Clone, Debug, PartialEq)]
pub struct DrawMetadata {
    pub bounding_volume: Option<BoundingSphere>,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    pub model_matrix: Option<Matrix4>,
    pub owner: Option<String>,
    pub pick_id: Option<String>,
    pub pick_metadata_allowed: bool,
    pub picked_metadata_info: Option<serde_json::Value>,
    pub debug_overlapping_frustums: u32,
    pub cull: bool,
    pub occlude: bool,
    pub execute_in_closest_frustum: bool,
    pub debug_show_bounding_volume: bool,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    pub pick_only: bool,
    pub depth_for_translucent_classification: bool,
}
impl Default for DrawMetadata {
    fn default() -> Self {
        Self {
            bounding_volume: None,
            oriented_bounding_box: None,
            model_matrix: None,
            owner: None,
            pick_id: None,
            pick_metadata_allowed: false,
            picked_metadata_info: None,
            debug_overlapping_frustums: 0,
            cull: true,
            occlude: true,
            execute_in_closest_frustum: false,
            debug_show_bounding_volume: false,
            cast_shadows: false,
            receive_shadows: false,
            pick_only: false,
            depth_for_translucent_classification: false,
        }
    }
}
#[derive(Clone, PartialEq)]
pub(crate) struct CommandSnapshot {
    metadata: DrawMetadata,
    vertex_array: usize,
    program: usize,
    primitive: u32,
    offset: usize,
    count: Option<usize>,
    instances: u32,
    framebuffer: usize,
    state: RenderState,
    pass: Option<Pass>,
    uniforms: Vec<(String, UniformValue)>,
    uniform_map: usize,
    textures: Vec<(String, usize)>,
}
pub(crate) struct CommandTracking {
    pub dirty: Cell<bool>,
    pub snapshot: RefCell<Option<CommandSnapshot>>,
}
impl Default for CommandTracking {
    fn default() -> Self {
        Self { dirty: Cell::new(true), snapshot: RefCell::new(None) }
    }
}
impl<'a, G: Gl> DrawCommand<'a, G> {
    fn snapshot(&self) -> CommandSnapshot {
        CommandSnapshot {
            metadata: self.metadata.clone(),
            vertex_array: self.vertex_array as *const _ as usize,
            program: self.shader_program as *const _ as usize,
            primitive: self.primitive_type as u32,
            offset: self.offset,
            count: self.count,
            instances: self.instance_count,
            framebuffer: self.framebuffer.as_ref().map_or(0, |f| Rc::as_ptr(f) as usize),
            state: self.render_state.clone(),
            pass: self.pass,
            uniforms: self.uniforms.clone(),
            uniform_map: self.uniform_map.as_ref().map_or(0, |p| Rc::as_ptr(p) as *const () as usize),
            textures: self.textures.iter().map(|(n, t)| (n.clone(), *t as *const _ as usize)).collect(),
        }
    }
    /// Rust fields are compared with the last clean snapshot. This catches both
    /// setter calls and direct edits to owned command data.
    pub fn dirty(&self) -> bool {
        if !self.tracking.dirty.get() && self.tracking.snapshot.borrow().as_ref() != Some(&self.snapshot()) {
            self.tracking.dirty.set(true);
        }
        self.tracking.dirty.get()
    }
    pub fn set_dirty(&self, dirty: bool) {
        if !dirty {
            *self.tracking.snapshot.borrow_mut() = Some(self.snapshot());
        }
        self.tracking.dirty.set(dirty);
    }
    pub fn shallow_clone(&self) -> Self {
        Self {
            vertex_array: self.vertex_array,
            shader_program: self.shader_program,
            primitive_type: self.primitive_type,
            offset: self.offset,
            count: self.count,
            instance_count: self.instance_count,
            render_state: self.render_state.clone(),
            framebuffer: self.framebuffer.clone(),
            pass: self.pass,
            uniforms: self.uniforms.clone(),
            uniform_map: self.uniform_map.clone(),
            textures: self.textures.clone(),
            metadata: self.metadata.clone(),
            last_dirty_time: 0.,
            derived_commands: HashMap::new(),
            tracking: CommandTracking::default(),
        }
    }
    pub fn clone_into(&self, result: &mut Self) {
        let derived = std::mem::take(&mut result.derived_commands);
        *result = self.shallow_clone();
        result.derived_commands = derived;
    }
}
pub(crate) fn empty_derived<'a, G: Gl>() -> HashMap<String, Rc<DrawCommand<'a, G>>> {
    HashMap::new()
}
