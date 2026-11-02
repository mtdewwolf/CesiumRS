//! CesiumJS `DebugInspector`: draws a command with its fragment color tinted
//! by `debugShowCommands` (a random color per command) and
//! `debugShowFrustums` (red, green and blue for the first three frustums the
//! command overlaps).
//!
//! Cesium caches debug programs by the source program's id. The port keys the
//! cache by the source program's vertex and fragment text, which identifies
//! the same programs. Cesium adds the two debug uniforms to the command's own
//! uniform map; the port wraps that map in the debug command instead, so the
//! command is left unchanged. A map that already has either debug uniform is
//! used as is, as in Cesium.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::core::color::ColorRandomOptions;
use crate::core::{Color, RuntimeError};
use crate::renderer::{
    Context, DrawCommand, Gl, PassState, ShaderCache, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformMap,
    UniformValue,
};

const COMMANDS_COLOR: &str = "debugShowCommandsColor";
const FRUSTUMS_COLOR: &str = "debugShowFrustumsColor";

/// The scene flags the debug uniforms read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugShowOptions {
    /// `scene.debugShowCommands`.
    pub debug_show_commands: bool,
    /// `scene.debugShowFrustums`.
    pub debug_show_frustums: bool,
}

pub struct DebugInspector<G: Gl> {
    cached_show_frustums_shaders: HashMap<(String, String), Rc<ShaderProgram<G>>>,
}

impl<G: Gl> Default for DebugInspector<G> {
    fn default() -> Self {
        Self::new()
    }
}

/// The debug fragment shader: every source's `main` renamed to
/// `czm_Debug_main`, then a new `main` that multiplies each `out_FragData_N`
/// target, in order of first appearance across the sources, or
/// `out_FragColor` when there are none, by both debug colors.
pub fn create_debug_show_frustums_fragment_shader(fs: &ShaderSource) -> ShaderSource {
    let mut fs = fs.clone();
    let mut targets: Vec<String> = Vec::new();
    let pattern = regex::Regex::new(r"out_FragData_(\d+)").expect("valid regex");
    fs.sources = fs
        .sources
        .iter()
        .map(|source| {
            let source = ShaderSource::replace_main(source, "czm_Debug_main");
            for capture in pattern.captures_iter(&source) {
                let target = capture[1].to_string();
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
            source
        })
        .collect();

    let mut new_main = String::new();
    new_main.push_str("uniform vec3 debugShowCommandsColor;\n");
    new_main.push_str("uniform vec3 debugShowFrustumsColor;\n");
    new_main.push_str("void main() \n{ \n    czm_Debug_main(); \n");
    if targets.is_empty() {
        new_main.push_str("    out_FragColor.rgb *= debugShowCommandsColor;\n");
        new_main.push_str("    out_FragColor.rgb *= debugShowFrustumsColor;\n");
    } else {
        for target in &targets {
            new_main.push_str(&format!("    out_FragData_{target}.rgb *= debugShowCommandsColor;\n"));
            new_main.push_str(&format!("    out_FragData_{target}.rgb *= debugShowFrustumsColor;\n"));
        }
    }
    new_main.push('}');
    fs.sources.push(new_main);
    fs
}

/// `debugShowFrustumsColor`: white when the scene does not show frustums,
/// otherwise red, green and blue for overlap bits 0, 1 and 2.
pub fn debug_show_frustums_color(show_frustums: bool, debug_overlapping_frustums: u32) -> Color {
    if !show_frustums {
        return Color::WHITE;
    }
    let bit = |b: u32| if debug_overlapping_frustums & (1 << b) != 0 { 1.0 } else { 0.0 };
    Color::new(bit(0), bit(1), bit(2), 1.0)
}

/// `debugShowCommandsColor`: white when the scene does not show commands,
/// otherwise the command's random color, chosen with `Color.fromRandom()`
/// the first time and kept on the command.
pub fn debug_show_commands_color<G: Gl>(show_commands: bool, command: &DrawCommand<'_, G>) -> Color {
    if !show_commands {
        return Color::WHITE;
    }
    let slot = &command.metadata.debug_color;
    slot.get().unwrap_or_else(|| {
        let color = Color::from_random(&ColorRandomOptions::default());
        slot.set(Some(color));
        color
    })
}

fn vec3(color: Color) -> UniformValue {
    UniformValue::Float(vec![color.red as f32, color.green as f32, color.blue as f32])
}

/// `createDebugShowFrustumsUniformMap`. The command's map is evaluated once
/// to find out whether it already sets a debug uniform.
pub fn create_debug_show_frustums_uniform_map<G: Gl>(
    options: DebugShowOptions,
    command: &DrawCommand<'_, G>,
) -> Result<UniformMap, RuntimeError> {
    let existing = command.uniform_map.clone();
    if let Some(map) = &existing {
        if map()?.iter().any(|(name, _)| name == COMMANDS_COLOR || name == FRUSTUMS_COLOR) {
            return Ok(map.clone());
        }
    }
    let commands_color = debug_show_commands_color(options.debug_show_commands, command);
    let frustums_color =
        debug_show_frustums_color(options.debug_show_frustums, command.metadata.debug_overlapping_frustums);
    Ok(Rc::new(move || {
        let mut values = match &existing {
            Some(map) => map()?,
            None => Vec::new(),
        };
        values.push((COMMANDS_COLOR.into(), vec3(commands_color)));
        values.push((FRUSTUMS_COLOR.into(), vec3(frustums_color)));
        Ok(values)
    }))
}

impl<G: Gl> DebugInspector<G> {
    pub fn new() -> Self {
        Self { cached_show_frustums_shaders: HashMap::new() }
    }

    /// The number of cached debug programs.
    pub fn number_of_cached_programs(&self) -> usize {
        self.cached_show_frustums_shaders.len()
    }

    /// The cached debug program for `program`, if it has been drawn.
    pub fn cached_program(&self, program: &ShaderProgram<G>) -> Option<Rc<ShaderProgram<G>>> {
        let key = (program.vertex_shader_source().to_string(), program.fragment_shader_source().to_string());
        self.cached_show_frustums_shaders.get(&key).cloned()
    }

    /// `createDebugShowFrustumsShaderProgram`: the source program's vertex
    /// shader, the debug fragment shader and the locations of its active
    /// attributes. The source program must come from a [`ShaderCache`],
    /// which keeps its `ShaderSource`s.
    fn debug_program(
        &mut self,
        shader_cache: &mut ShaderCache<G>,
        program: &ShaderProgram<G>,
    ) -> Result<Rc<ShaderProgram<G>>, RuntimeError> {
        let key = (program.vertex_shader_source().to_string(), program.fragment_shader_source().to_string());
        if let Some(cached) = self.cached_show_frustums_shaders.get(&key) {
            return Ok(cached.clone());
        }
        let source = program.source_options().ok_or_else(|| {
            RuntimeError::new("DebugInspector needs a shader program created through the shader cache")
        })?;
        let attribute_locations: BTreeMap<String, u32> = program
            .vertex_attributes()
            .iter()
            .filter_map(|a| source.attribute_locations.get(&a.name).map(|&location| (a.name.clone(), location)))
            .collect();
        let debug = shader_cache.get_shader_program(&ShaderProgramOptions {
            vertex_shader_source: source.vertex_shader_source.clone(),
            fragment_shader_source: create_debug_show_frustums_fragment_shader(&source.fragment_shader_source),
            attribute_locations,
        })?;
        self.cached_show_frustums_shaders.insert(key, debug.clone());
        Ok(debug)
    }

    /// `executeDebugShowFrustumsCommand`: draws a shallow clone of `command`
    /// with the debug program and uniforms.
    pub fn execute_debug_show_frustums_command(
        &mut self,
        options: DebugShowOptions,
        context: &Context<G>,
        shader_cache: &mut ShaderCache<G>,
        command: &DrawCommand<'_, G>,
        pass: &PassState<G>,
    ) -> Result<(), RuntimeError> {
        let program = self.debug_program(shader_cache, command.shader_program)?;
        let uniform_map = create_debug_show_frustums_uniform_map(options, command)?;
        let mut debug = command.shallow_clone();
        debug.shader_program = &program;
        debug.uniform_map = Some(uniform_map);
        debug.execute(context, pass)
    }
}
