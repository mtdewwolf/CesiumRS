//! CesiumJS `Scene/Model/ImageryPipelineStage`: configures shader code,
//! uniforms, and attributes for draping imagery over 3D Tiles and model primitives.

use super::{ImageryFlags, ImageryInput, PrimitiveRenderResources};
use crate::core::{Cartesian4, Rectangle, RuntimeError};
use crate::renderer::{Gl, ShaderBuilder, ShaderDestination, UniformValue};
use std::rc::Rc;

/// Generates shader structures, uniforms, and texture coordinates for draped imagery.
pub struct ImageryPipelineStage;

impl ImageryPipelineStage {
    /// Computes offset and scale `(offsetX, offsetY, scaleX, scaleY)` mapping native imagery bounds to geometry bounds.
    pub fn compute_texture_translation_and_scale_from_native(
        native_bounding_rectangle: &Rectangle,
        native_imagery_rectangle: &Rectangle,
    ) -> Cartesian4 {
        let inv_w = 1.0 / native_imagery_rectangle.width();
        let inv_h = 1.0 / native_imagery_rectangle.height();
        let delta_west = native_bounding_rectangle.west - native_imagery_rectangle.west;
        let delta_south = native_bounding_rectangle.south - native_imagery_rectangle.south;
        let offset_x = delta_west * inv_w;
        let offset_y = delta_south * inv_h;
        let scale_x = native_bounding_rectangle.width() * inv_w;
        let scale_y = native_bounding_rectangle.height() * inv_h;
        Cartesian4::new(offset_x, offset_y, scale_x, scale_y)
    }

    /// Computes index mapping from source elements to target elements.
    pub fn compute_index_mapping<T: PartialEq>(source: &[T], target: &[T]) -> Vec<usize> {
        source.iter().map(|s| target.iter().position(|t| t == s).unwrap_or(0)).collect()
    }

    /// Computes unified `ImageryFlags` from a collection of per-layer flags.
    pub fn compute_imagery_flags(layer_flags: &[ImageryFlags]) -> ImageryFlags {
        let mut flags = ImageryFlags::default();
        for lf in layer_flags {
            flags.alpha |= lf.alpha;
            flags.brightness |= lf.brightness;
            flags.contrast |= lf.contrast;
            flags.hue |= lf.hue;
            flags.saturation |= lf.saturation;
            flags.gamma |= lf.gamma;
            flags.color_to_alpha |= lf.color_to_alpha;
        }
        flags
    }

    /// Defines uniforms required for draped imagery on the shader builder.
    pub fn define_uniforms(shader_builder: &mut ShaderBuilder, imagery_flags: &ImageryFlags) {
        shader_builder.add_uniform(
            "sampler2D",
            "u_imageryTextures[IMAGERY_TEXTURE_UNITS]",
            ShaderDestination::FRAGMENT,
        );
        shader_builder.add_uniform(
            "vec4",
            "u_imageryTextureCoordinateRectangle[IMAGERY_TEXTURE_UNITS]",
            ShaderDestination::FRAGMENT,
        );
        shader_builder.add_uniform(
            "vec4",
            "u_imageryTextureTranslationAndScale[IMAGERY_TEXTURE_UNITS]",
            ShaderDestination::FRAGMENT,
        );

        if imagery_flags.alpha {
            shader_builder.add_uniform(
                "float",
                "u_imageryTextureAlpha[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
        if imagery_flags.brightness {
            shader_builder.add_uniform(
                "float",
                "u_imageryTextureBrightness[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
        if imagery_flags.contrast {
            shader_builder.add_uniform(
                "float",
                "u_imageryTextureContrast[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
        if imagery_flags.hue {
            shader_builder.add_uniform(
                "float",
                "u_imageryTextureHue[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
        if imagery_flags.saturation {
            shader_builder.add_uniform(
                "float",
                "u_imageryTextureSaturation[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
        if imagery_flags.gamma {
            shader_builder.add_uniform(
                "float",
                "u_imageryTextureOneOverGamma[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
        if imagery_flags.color_to_alpha {
            shader_builder.add_uniform(
                "vec4",
                "u_imageryTextureColorToAlpha[IMAGERY_TEXTURE_UNITS]",
                ShaderDestination::FRAGMENT,
            );
        }
    }

    /// Processes imagery inputs and configures the primitive render resources.
    pub fn process_imagery_inputs<G: Gl>(
        render_resources: &mut PrimitiveRenderResources<G>,
        imagery_inputs: &[ImageryInput<G>],
        num_imagery_tex_coord_attributes: usize,
        imagery_flags: &ImageryFlags,
    ) -> Result<(), RuntimeError> {
        if imagery_inputs.is_empty() {
            return Ok(());
        }

        let num_textures = imagery_inputs.len().min(10);
        let inputs = &imagery_inputs[..num_textures];

        let shader_builder = &mut render_resources.shader_builder;

        shader_builder.add_define("HAS_IMAGERY", None, ShaderDestination::BOTH);
        shader_builder.add_define(&format!("IMAGERY_TEXTURE_UNITS {num_textures}"), None, ShaderDestination::BOTH);

        // Add attributes and varyings
        for i in 0..num_imagery_tex_coord_attributes {
            shader_builder.add_attribute("vec2", &format!("a_imagery_texCoord_{i}"));
            shader_builder.add_varying("vec2", &format!("v_imagery_texCoord_{i}"), None);
        }

        let mut init_lines = Vec::new();
        for i in 0..num_imagery_tex_coord_attributes {
            init_lines.push(format!("v_imagery_texCoord_{i} = a_imagery_texCoord_{i};"));
        }
        let init_lines_str: Vec<&str> = init_lines.iter().map(String::as_str).collect();

        shader_builder.add_function(
            "initializeImageryAttributes",
            "void initializeImageryAttributes()",
            ShaderDestination::VERTEX,
        );
        shader_builder.add_function_lines("initializeImageryAttributes", &init_lines_str)?;

        // Define uniforms
        Self::define_uniforms(shader_builder, imagery_flags);

        // Build sampleAndBlend function
        Self::build_sample_and_blend_function(shader_builder, imagery_flags)?;

        // Build blendBaseColorWithImagery function
        Self::create_main_imagery_shader(shader_builder, inputs, imagery_flags)?;

        // Populate uniform map
        let rect_values: Vec<f32> = inputs
            .iter()
            .flat_map(|inp| {
                [
                    inp.texture_coordinate_rectangle.x as f32,
                    inp.texture_coordinate_rectangle.y as f32,
                    inp.texture_coordinate_rectangle.z as f32,
                    inp.texture_coordinate_rectangle.w as f32,
                ]
            })
            .collect();
        render_resources.uniform_map.insert(
            "u_imageryTextureCoordinateRectangle".into(),
            Rc::new(move || UniformValue::FloatArray { components: 4, values: rect_values.clone() }),
        );

        let trans_scale_values: Vec<f32> = inputs
            .iter()
            .flat_map(|inp| {
                [
                    inp.texture_translation_and_scale.x as f32,
                    inp.texture_translation_and_scale.y as f32,
                    inp.texture_translation_and_scale.z as f32,
                    inp.texture_translation_and_scale.w as f32,
                ]
            })
            .collect();
        render_resources.uniform_map.insert(
            "u_imageryTextureTranslationAndScale".into(),
            Rc::new(move || UniformValue::FloatArray { components: 4, values: trans_scale_values.clone() }),
        );

        let alpha_vals: Vec<f32> = inputs.iter().map(|inp| inp.alpha).collect();
        render_resources
            .uniform_map
            .insert("u_imageryTextureAlpha".into(), Rc::new(move || UniformValue::Float(alpha_vals.clone())));

        let brightness_vals: Vec<f32> = inputs.iter().map(|inp| inp.brightness).collect();
        render_resources
            .uniform_map
            .insert("u_imageryTextureBrightness".into(), Rc::new(move || UniformValue::Float(brightness_vals.clone())));

        let contrast_vals: Vec<f32> = inputs.iter().map(|inp| inp.contrast).collect();
        render_resources
            .uniform_map
            .insert("u_imageryTextureContrast".into(), Rc::new(move || UniformValue::Float(contrast_vals.clone())));

        let hue_vals: Vec<f32> = inputs.iter().map(|inp| inp.hue).collect();
        render_resources
            .uniform_map
            .insert("u_imageryTextureHue".into(), Rc::new(move || UniformValue::Float(hue_vals.clone())));

        let sat_vals: Vec<f32> = inputs.iter().map(|inp| inp.saturation).collect();
        render_resources
            .uniform_map
            .insert("u_imageryTextureSaturation".into(), Rc::new(move || UniformValue::Float(sat_vals.clone())));

        let gamma_vals: Vec<f32> = inputs.iter().map(|inp| inp.one_over_gamma).collect();
        render_resources
            .uniform_map
            .insert("u_imageryTextureOneOverGamma".into(), Rc::new(move || UniformValue::Float(gamma_vals.clone())));

        let c2a_vals: Vec<f32> = inputs
            .iter()
            .flat_map(|inp| {
                if let Some(c) = inp.color_to_alpha {
                    [c.x as f32, c.y as f32, c.z as f32, c.w as f32]
                } else {
                    [0.0, 0.0, 0.0, -1.0]
                }
            })
            .collect();
        render_resources.uniform_map.insert(
            "u_imageryTextureColorToAlpha".into(),
            Rc::new(move || UniformValue::FloatArray { components: 4, values: c2a_vals.clone() }),
        );

        Ok(())
    }

    /// Builds the `sampleAndBlend` GLSL fragment shader function according to active imagery flags.
    pub fn build_sample_and_blend_function(
        shader_builder: &mut ShaderBuilder,
        imagery_flags: &ImageryFlags,
    ) -> Result<(), RuntimeError> {
        let mut params = vec![
            "vec4 previousColor",
            "sampler2D textureToSample",
            "vec2 textureCoordinates",
            "vec4 textureCoordinateRectangle",
            "vec4 textureCoordinateTranslationAndScale",
        ];
        if imagery_flags.alpha {
            params.push("float textureAlpha");
        }
        if imagery_flags.brightness {
            params.push("float textureBrightness");
        }
        if imagery_flags.contrast {
            params.push("float textureContrast");
        }
        if imagery_flags.hue {
            params.push("float textureHue");
        }
        if imagery_flags.saturation {
            params.push("float textureSaturation");
        }
        if imagery_flags.gamma {
            params.push("float textureOneOverGamma");
        }
        if imagery_flags.color_to_alpha {
            params.push("vec4 colorToAlpha");
        }

        let signature = format!("vec4 sampleAndBlend({})", params.join(", "));
        shader_builder.add_function("sampleAndBlend", &signature, ShaderDestination::FRAGMENT);

        let mut lines = vec!["float effectiveAlpha = 1.0;".to_string()];
        if imagery_flags.alpha {
            lines.push("effectiveAlpha = textureAlpha;".to_string());
        }
        lines.push("if (textureCoordinates.x < textureCoordinateRectangle.x) effectiveAlpha = 0.0;".to_string());
        lines.push("if (textureCoordinates.x > textureCoordinateRectangle.z) effectiveAlpha = 0.0;".to_string());
        lines.push("if (textureCoordinates.y < textureCoordinateRectangle.y) effectiveAlpha = 0.0;".to_string());
        lines.push("if (textureCoordinates.y > textureCoordinateRectangle.w) effectiveAlpha = 0.0;".to_string());
        lines.push("vec2 translation = textureCoordinateTranslationAndScale.xy;".to_string());
        lines.push("vec2 scale = textureCoordinateTranslationAndScale.zw;".to_string());
        lines.push("vec2 effectiveTextureCoordinates = textureCoordinates * scale + translation;".to_string());
        lines.push("vec4 value = texture(textureToSample, effectiveTextureCoordinates);".to_string());
        lines.push("value = czm_srgbToLinear(value);".to_string());
        lines.push("vec3 color = value.rgb;".to_string());
        lines.push("float alpha = value.a;".to_string());

        if imagery_flags.color_to_alpha {
            lines.push("vec3 colorDiff = abs(color.rgb - colorToAlpha.rgb);".to_string());
            lines.push("colorDiff.r = czm_maximumComponent(colorDiff);".to_string());
            lines.push("alpha = czm_branchFreeTernary(colorDiff.r < colorToAlpha.a, 0.0, alpha);".to_string());
        }

        if imagery_flags.gamma {
            lines.push("color = pow(color, vec3(textureOneOverGamma));".to_string());
        } else {
            lines.push("vec4 tempColor = czm_gammaCorrect(vec4(color, alpha));".to_string());
            lines.push("color = tempColor.rgb;".to_string());
            lines.push("alpha = tempColor.a;".to_string());
        }

        if imagery_flags.brightness {
            lines.push("color = mix(vec3(0.0), color, textureBrightness);".to_string());
        }
        if imagery_flags.contrast {
            lines.push("color = mix(vec3(0.5), color, textureContrast);".to_string());
        }
        if imagery_flags.hue {
            lines.push("color = czm_hue(color, textureHue);".to_string());
        }
        if imagery_flags.saturation {
            lines.push("color = czm_saturation(color, textureSaturation);".to_string());
        }

        lines.push("float sourceAlpha = alpha * effectiveAlpha;".to_string());
        lines.push("float outAlpha = mix(previousColor.a, 1.0, sourceAlpha);".to_string());
        lines.push("outAlpha += sign(outAlpha) - 1.0;".to_string());
        lines.push(
            "vec3 outColor = mix(previousColor.rgb * previousColor.a, color, sourceAlpha) / outAlpha;".to_string(),
        );
        lines.push("vec4 result = vec4(outColor, max(outAlpha, 0.0));".to_string());
        lines.push("return result;".to_string());

        let lines_str: Vec<&str> = lines.iter().map(String::as_str).collect();
        shader_builder.add_function_lines("sampleAndBlend", &lines_str)?;
        Ok(())
    }

    /// Builds the `blendBaseColorWithImagery` GLSL fragment shader function for texture coordinate indices.
    pub fn create_main_imagery_shader_from_indices(
        shader_builder: &mut ShaderBuilder,
        tex_coord_indices: &[usize],
        imagery_flags: &ImageryFlags,
    ) -> Result<(), RuntimeError> {
        let signature = "vec4 blendBaseColorWithImagery(vec4 baseColorWithAlpha)";
        shader_builder.add_function("blendBaseColorWithImagery", signature, ShaderDestination::FRAGMENT);

        let mut lines = vec!["vec4 blendedBaseColor = baseColorWithAlpha;".to_string()];

        for (i, &tex_coord_idx) in tex_coord_indices.iter().enumerate() {
            let mut args = vec![
                "blendedBaseColor".to_string(),
                format!("u_imageryTextures[{i}]"),
                format!("v_imagery_texCoord_{tex_coord_idx}"),
                format!("u_imageryTextureCoordinateRectangle[{i}]"),
                format!("u_imageryTextureTranslationAndScale[{i}]"),
            ];
            if imagery_flags.alpha {
                args.push(format!("u_imageryTextureAlpha[{i}]"));
            }
            if imagery_flags.brightness {
                args.push(format!("u_imageryTextureBrightness[{i}]"));
            }
            if imagery_flags.contrast {
                args.push(format!("u_imageryTextureContrast[{i}]"));
            }
            if imagery_flags.hue {
                args.push(format!("u_imageryTextureHue[{i}]"));
            }
            if imagery_flags.saturation {
                args.push(format!("u_imageryTextureSaturation[{i}]"));
            }
            if imagery_flags.gamma {
                args.push(format!("u_imageryTextureOneOverGamma[{i}]"));
            }
            if imagery_flags.color_to_alpha {
                args.push(format!("u_imageryTextureColorToAlpha[{i}]"));
            }

            lines.push(format!("blendedBaseColor = sampleAndBlend({});", args.join(", ")));
        }

        lines.push("return blendedBaseColor;".to_string());

        let lines_str: Vec<&str> = lines.iter().map(String::as_str).collect();
        shader_builder.add_function_lines("blendBaseColorWithImagery", &lines_str)?;
        Ok(())
    }

    fn create_main_imagery_shader<G: Gl>(
        shader_builder: &mut ShaderBuilder,
        inputs: &[ImageryInput<G>],
        imagery_flags: &ImageryFlags,
    ) -> Result<(), RuntimeError> {
        let indices: Vec<usize> = inputs.iter().map(|inp| inp.imagery_tex_coord_attribute_set_index as usize).collect();
        Self::create_main_imagery_shader_from_indices(shader_builder, &indices, imagery_flags)
    }
}
