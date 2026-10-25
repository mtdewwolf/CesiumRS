//! CesiumJS `Scene/Model/MaterialPipelineStage`: processes PBR metallic-roughness,
//! specular-glossiness, specular, anisotropy, clearcoat, unlit, alpha, culling,
//! and point-diameter material properties into shader defines, uniforms, lighting
//! options, and render state.

use super::{
    model_utility, LightingModel, ModelTextureUniformProviders, ModelUniformProviders, PrimitiveRenderResources,
};
use crate::core::Matrix3;
use crate::renderer::{Context, Gl, Pass, ShaderBuilder, ShaderDestination, Texture, UniformValue};
use crate::scene::{model_components as mc, AlphaMode, VertexAttributeSemantic};
use crate::shaders;
use std::rc::Rc;

pub fn process<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    context: &Context<G>,
    pixel_ratio: f64,
) {
    let default_material = mc::Material::default();
    let material = primitive.material.as_ref().unwrap_or(&default_material);

    let (has_classification, disable_point_cloud_normals, back_face_culling) = {
        let model = resources.model.borrow();
        (
            model.classification_type.is_some(),
            model.point_cloud_shading_normal_shading == Some(false),
            model.back_face_culling,
        )
    };
    let disable_textures = has_classification;

    let default_texture = context.default_texture().ok();
    let default_normal_texture = context.default_normal_texture().ok();
    let default_emissive_texture = context.default_emissive_texture().ok();

    process_material_uniforms(
        material,
        &mut resources.pipeline.uniform_map,
        &mut resources.texture_uniform_map,
        &mut resources.pipeline.shader_builder,
        default_texture.clone(),
        default_normal_texture,
        default_emissive_texture,
        disable_textures,
    );

    if let Some(specular_glossiness) = &material.specular_glossiness {
        process_specular_glossiness_uniforms(
            specular_glossiness,
            &mut resources.pipeline.uniform_map,
            &mut resources.texture_uniform_map,
            &mut resources.pipeline.shader_builder,
            default_texture,
            disable_textures,
        );
    } else {
        if let Some(specular) = &material.specular {
            process_specular_uniforms(
                specular,
                &mut resources.pipeline.uniform_map,
                &mut resources.texture_uniform_map,
                &mut resources.pipeline.shader_builder,
                default_texture.clone(),
                disable_textures,
            );
        }
        if let Some(anisotropy) = &material.anisotropy {
            process_anisotropy_uniforms(
                anisotropy,
                &mut resources.pipeline.uniform_map,
                &mut resources.texture_uniform_map,
                &mut resources.pipeline.shader_builder,
                default_texture.clone(),
                disable_textures,
            );
        }
        if let Some(clearcoat) = &material.clearcoat {
            process_clearcoat_uniforms(
                clearcoat,
                &mut resources.pipeline.uniform_map,
                &mut resources.texture_uniform_map,
                &mut resources.pipeline.shader_builder,
                default_texture.clone(),
                disable_textures,
            );
        }
        process_metallic_roughness_uniforms(
            &material.metallic_roughness,
            &mut resources.pipeline.uniform_map,
            &mut resources.texture_uniform_map,
            &mut resources.pipeline.shader_builder,
            default_texture,
            disable_textures,
        );
    }

    let has_normals = model_utility::get_attribute_by_semantic(
        &primitive.attributes,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Normal),
        None,
    )
    .is_some();

    if material.unlit || !has_normals || has_classification || disable_point_cloud_normals {
        resources.lighting_options.lighting_model = LightingModel::Unlit;
    } else {
        resources.lighting_options.lighting_model = LightingModel::Pbr;
    }

    let cull = back_face_culling && !material.double_sided;
    resources.render_state_options.cull.enabled = cull;

    {
        let mut alpha_options = resources.alpha_options.borrow_mut();
        if material.alpha_mode == AlphaMode::Blend {
            alpha_options.pass = Some(Pass::Translucent);
        } else if material.alpha_mode == AlphaMode::Mask {
            alpha_options.alpha_cutoff = material.alpha_cutoff;
        }
    }

    if let Some(point_diameter) = material.point_diameter {
        resources.shader_builder.add_define("HAS_POINT_DIAMETER", None, ShaderDestination::BOTH);
        resources.shader_builder.add_uniform("float", "u_pointDiameter", ShaderDestination::VERTEX);
        resources.uniform_map.insert(
            "u_pointDiameter".into(),
            Rc::new(move || UniformValue::Float(vec![(point_diameter * pixel_ratio) as f32])),
        );
    }

    resources.shader_builder.add_fragment_lines(&[shaders::model::MATERIAL_STAGE_FS.source]);

    if material.double_sided {
        resources.shader_builder.add_define("HAS_DOUBLE_SIDED_MATERIAL", None, ShaderDestination::BOTH);
    }
}

pub fn process_texture_transform<G: Gl + 'static>(
    shader_builder: &mut ShaderBuilder,
    uniform_map: &mut ModelUniformProviders,
    texture_reader: &mc::TextureReader<G>,
    uniform_name: &str,
    define_name: &str,
) {
    let transform_define = format!("HAS_{define_name}_TEXTURE_TRANSFORM");
    shader_builder.add_define(&transform_define, None, ShaderDestination::FRAGMENT);

    let transform_uniform_name = format!("{uniform_name}Transform");
    shader_builder.add_uniform("mat3", &transform_uniform_name, ShaderDestination::FRAGMENT);
    let transform = texture_reader.transform.unwrap_or(Matrix3::IDENTITY);
    uniform_map.insert(
        transform_uniform_name,
        Rc::new(move || UniformValue::Matrix { dimension: 3, values: transform.0.iter().map(|&v| v as f32).collect() }),
    );
}

pub fn process_texture_scale<G: Gl + 'static>(
    shader_builder: &mut ShaderBuilder,
    uniform_map: &mut ModelUniformProviders,
    texture_reader: &mc::TextureReader<G>,
    uniform_name: &str,
    define_name: &str,
) {
    let scale_define = format!("HAS_{define_name}_TEXTURE_SCALE");
    shader_builder.add_define(&scale_define, None, ShaderDestination::FRAGMENT);

    let scale_uniform_name = format!("{uniform_name}Scale");
    shader_builder.add_uniform("float", &scale_uniform_name, ShaderDestination::FRAGMENT);
    let scale = texture_reader.scale.unwrap_or(1.0);
    uniform_map.insert(scale_uniform_name, Rc::new(move || UniformValue::Float(vec![scale as f32])));
}

pub fn process_texture<G: Gl + 'static>(
    shader_builder: &mut ShaderBuilder,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    texture_reader: &mc::TextureReader<G>,
    uniform_name: &str,
    define_name: &str,
    default_texture: Option<Rc<Texture<G>>>,
) {
    shader_builder.add_uniform("sampler2D", uniform_name, ShaderDestination::FRAGMENT);
    let tex = texture_reader.texture.clone();
    texture_uniform_map
        .insert(uniform_name.to_string(), Rc::new(move || tex.clone().or_else(|| default_texture.clone())));

    let texture_define = format!("HAS_{define_name}_TEXTURE");
    shader_builder.add_define(&texture_define, None, ShaderDestination::FRAGMENT);

    let tex_coord_index = texture_reader.tex_coord as u32;
    let tex_coord_varying = format!("v_texCoord_{tex_coord_index}");
    let tex_coord_define = format!("TEXCOORD_{define_name}");
    shader_builder.add_define(&tex_coord_define, Some(&tex_coord_varying), ShaderDestination::FRAGMENT);

    if let Some(texture_transform) = texture_reader.transform {
        if texture_transform != Matrix3::IDENTITY {
            process_texture_transform(shader_builder, uniform_map, texture_reader, uniform_name, define_name);
        }
    }

    if let Some(scale) = texture_reader.scale {
        if scale != 1.0 {
            process_texture_scale(shader_builder, uniform_map, texture_reader, uniform_name, define_name);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_material_uniforms<G: Gl + 'static>(
    material: &mc::Material<G>,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    shader_builder: &mut ShaderBuilder,
    default_texture: Option<Rc<Texture<G>>>,
    default_normal_texture: Option<Rc<Texture<G>>>,
    default_emissive_texture: Option<Rc<Texture<G>>>,
    disable_textures: bool,
) {
    if material.emissive_factor != mc::Material::<G>::DEFAULT_EMISSIVE_FACTOR {
        shader_builder.add_uniform("vec3", "u_emissiveFactor", ShaderDestination::FRAGMENT);
        let emissive_factor = material.emissive_factor;
        uniform_map.insert(
            "u_emissiveFactor".into(),
            Rc::new(move || {
                UniformValue::Float(vec![emissive_factor.x as f32, emissive_factor.y as f32, emissive_factor.z as f32])
            }),
        );
        shader_builder.add_define("HAS_EMISSIVE_FACTOR", None, ShaderDestination::FRAGMENT);

        if let Some(emissive_texture) = &material.emissive_texture {
            if !disable_textures {
                process_texture(
                    shader_builder,
                    uniform_map,
                    texture_uniform_map,
                    emissive_texture,
                    "u_emissiveTexture",
                    "EMISSIVE",
                    default_emissive_texture,
                );
            }
        }
    }

    if let Some(normal_texture) = &material.normal_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                normal_texture,
                "u_normalTexture",
                "NORMAL",
                default_normal_texture,
            );
        }
    }

    if let Some(occlusion_texture) = &material.occlusion_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                occlusion_texture,
                "u_occlusionTexture",
                "OCCLUSION",
                default_texture,
            );
        }
    }
}

fn process_specular_glossiness_uniforms<G: Gl + 'static>(
    specular_glossiness: &mc::SpecularGlossiness<G>,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    shader_builder: &mut ShaderBuilder,
    default_texture: Option<Rc<Texture<G>>>,
    disable_textures: bool,
) {
    shader_builder.add_define("USE_SPECULAR_GLOSSINESS", None, ShaderDestination::FRAGMENT);

    if let Some(diffuse_texture) = &specular_glossiness.diffuse_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                diffuse_texture,
                "u_diffuseTexture",
                "DIFFUSE",
                default_texture.clone(),
            );
        }
    }

    if specular_glossiness.diffuse_factor != mc::SpecularGlossiness::<G>::DEFAULT_DIFFUSE_FACTOR {
        shader_builder.add_uniform("vec4", "u_diffuseFactor", ShaderDestination::FRAGMENT);
        let diffuse_factor = specular_glossiness.diffuse_factor;
        uniform_map.insert(
            "u_diffuseFactor".into(),
            Rc::new(move || {
                UniformValue::Float(vec![
                    diffuse_factor.x as f32,
                    diffuse_factor.y as f32,
                    diffuse_factor.z as f32,
                    diffuse_factor.w as f32,
                ])
            }),
        );
        shader_builder.add_define("HAS_DIFFUSE_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if let Some(specular_glossiness_texture) = &specular_glossiness.specular_glossiness_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                specular_glossiness_texture,
                "u_specularGlossinessTexture",
                "SPECULAR_GLOSSINESS",
                default_texture,
            );
        }
    }

    if specular_glossiness.specular_factor != mc::SpecularGlossiness::<G>::DEFAULT_SPECULAR_FACTOR {
        shader_builder.add_uniform("vec3", "u_legacySpecularFactor", ShaderDestination::FRAGMENT);
        let specular_factor = specular_glossiness.specular_factor;
        uniform_map.insert(
            "u_legacySpecularFactor".into(),
            Rc::new(move || {
                UniformValue::Float(vec![specular_factor.x as f32, specular_factor.y as f32, specular_factor.z as f32])
            }),
        );
        shader_builder.add_define("HAS_LEGACY_SPECULAR_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if specular_glossiness.glossiness_factor != mc::SpecularGlossiness::<G>::DEFAULT_GLOSSINESS_FACTOR {
        shader_builder.add_uniform("float", "u_glossinessFactor", ShaderDestination::FRAGMENT);
        let glossiness_factor = specular_glossiness.glossiness_factor;
        uniform_map
            .insert("u_glossinessFactor".into(), Rc::new(move || UniformValue::Float(vec![glossiness_factor as f32])));
        shader_builder.add_define("HAS_GLOSSINESS_FACTOR", None, ShaderDestination::FRAGMENT);
    }
}

fn process_specular_uniforms<G: Gl + 'static>(
    specular: &mc::Specular<G>,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    shader_builder: &mut ShaderBuilder,
    default_texture: Option<Rc<Texture<G>>>,
    disable_textures: bool,
) {
    shader_builder.add_define("USE_SPECULAR", None, ShaderDestination::FRAGMENT);

    if let Some(specular_texture) = &specular.specular_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                specular_texture,
                "u_specularTexture",
                "SPECULAR",
                default_texture.clone(),
            );
        }
    }

    if specular.specular_factor != mc::Specular::<G>::DEFAULT_SPECULAR_FACTOR {
        shader_builder.add_uniform("float", "u_specularFactor", ShaderDestination::FRAGMENT);
        let specular_factor = specular.specular_factor;
        uniform_map
            .insert("u_specularFactor".into(), Rc::new(move || UniformValue::Float(vec![specular_factor as f32])));
        shader_builder.add_define("HAS_SPECULAR_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if let Some(specular_color_texture) = &specular.specular_color_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                specular_color_texture,
                "u_specularColorTexture",
                "SPECULAR_COLOR",
                default_texture,
            );
        }
    }

    if specular.specular_color_factor != mc::Specular::<G>::DEFAULT_SPECULAR_COLOR_FACTOR {
        shader_builder.add_uniform("vec3", "u_specularColorFactor", ShaderDestination::FRAGMENT);
        let specular_color_factor = specular.specular_color_factor;
        uniform_map.insert(
            "u_specularColorFactor".into(),
            Rc::new(move || {
                UniformValue::Float(vec![
                    specular_color_factor.x as f32,
                    specular_color_factor.y as f32,
                    specular_color_factor.z as f32,
                ])
            }),
        );
        shader_builder.add_define("HAS_SPECULAR_COLOR_FACTOR", None, ShaderDestination::FRAGMENT);
    }
}

fn process_anisotropy_uniforms<G: Gl + 'static>(
    anisotropy: &mc::Anisotropy<G>,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    shader_builder: &mut ShaderBuilder,
    default_texture: Option<Rc<Texture<G>>>,
    disable_textures: bool,
) {
    shader_builder.add_define("USE_ANISOTROPY", None, ShaderDestination::FRAGMENT);

    if let Some(anisotropy_texture) = &anisotropy.anisotropy_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                anisotropy_texture,
                "u_anisotropyTexture",
                "ANISOTROPY",
                default_texture,
            );
        }
    }

    let cos_rotation = anisotropy.anisotropy_rotation.cos();
    let sin_rotation = anisotropy.anisotropy_rotation.sin();
    let strength = anisotropy.anisotropy_strength;
    shader_builder.add_uniform("vec3", "u_anisotropy", ShaderDestination::FRAGMENT);
    uniform_map.insert(
        "u_anisotropy".into(),
        Rc::new(move || UniformValue::Float(vec![cos_rotation as f32, sin_rotation as f32, strength as f32])),
    );
}

fn process_clearcoat_uniforms<G: Gl + 'static>(
    clearcoat: &mc::Clearcoat<G>,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    shader_builder: &mut ShaderBuilder,
    default_texture: Option<Rc<Texture<G>>>,
    disable_textures: bool,
) {
    shader_builder.add_define("USE_CLEARCOAT", None, ShaderDestination::FRAGMENT);

    if clearcoat.clearcoat_factor != mc::Clearcoat::<G>::DEFAULT_CLEARCOAT_FACTOR {
        shader_builder.add_uniform("float", "u_clearcoatFactor", ShaderDestination::FRAGMENT);
        let clearcoat_factor = clearcoat.clearcoat_factor;
        uniform_map
            .insert("u_clearcoatFactor".into(), Rc::new(move || UniformValue::Float(vec![clearcoat_factor as f32])));
        shader_builder.add_define("HAS_CLEARCOAT_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if let Some(clearcoat_texture) = &clearcoat.clearcoat_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                clearcoat_texture,
                "u_clearcoatTexture",
                "CLEARCOAT",
                default_texture.clone(),
            );
        }
    }

    // Matches upstream MaterialPipelineStage.js line 603:
    // `defined(clearcoatRoughnessFactor) && clearcoatFactor !== Clearcoat.DEFAULT_CLEARCOAT_ROUGHNESS_FACTOR`
    if clearcoat.clearcoat_factor != mc::Clearcoat::<G>::DEFAULT_CLEARCOAT_ROUGHNESS_FACTOR {
        shader_builder.add_uniform("float", "u_clearcoatRoughnessFactor", ShaderDestination::FRAGMENT);
        let clearcoat_roughness_factor = clearcoat.clearcoat_roughness_factor;
        uniform_map.insert(
            "u_clearcoatRoughnessFactor".into(),
            Rc::new(move || UniformValue::Float(vec![clearcoat_roughness_factor as f32])),
        );
        shader_builder.add_define("HAS_CLEARCOAT_ROUGHNESS_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if let Some(clearcoat_roughness_texture) = &clearcoat.clearcoat_roughness_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                clearcoat_roughness_texture,
                "u_clearcoatRoughnessTexture",
                "CLEARCOAT_ROUGHNESS",
                default_texture.clone(),
            );
        }
    }

    if let Some(clearcoat_normal_texture) = &clearcoat.clearcoat_normal_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                clearcoat_normal_texture,
                "u_clearcoatNormalTexture",
                "CLEARCOAT_NORMAL",
                default_texture,
            );
        }
    }
}

fn process_metallic_roughness_uniforms<G: Gl + 'static>(
    metallic_roughness: &mc::MetallicRoughness<G>,
    uniform_map: &mut ModelUniformProviders,
    texture_uniform_map: &mut ModelTextureUniformProviders<G>,
    shader_builder: &mut ShaderBuilder,
    default_texture: Option<Rc<Texture<G>>>,
    disable_textures: bool,
) {
    shader_builder.add_define("USE_METALLIC_ROUGHNESS", None, ShaderDestination::FRAGMENT);

    if let Some(base_color_texture) = &metallic_roughness.base_color_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                base_color_texture,
                "u_baseColorTexture",
                "BASE_COLOR",
                default_texture.clone(),
            );
        }
    }

    if metallic_roughness.base_color_factor != mc::MetallicRoughness::<G>::DEFAULT_BASE_COLOR_FACTOR {
        shader_builder.add_uniform("vec4", "u_baseColorFactor", ShaderDestination::FRAGMENT);
        let base_color_factor = metallic_roughness.base_color_factor;
        uniform_map.insert(
            "u_baseColorFactor".into(),
            Rc::new(move || {
                UniformValue::Float(vec![
                    base_color_factor.x as f32,
                    base_color_factor.y as f32,
                    base_color_factor.z as f32,
                    base_color_factor.w as f32,
                ])
            }),
        );
        shader_builder.add_define("HAS_BASE_COLOR_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if let Some(metallic_roughness_texture) = &metallic_roughness.metallic_roughness_texture {
        if !disable_textures {
            process_texture(
                shader_builder,
                uniform_map,
                texture_uniform_map,
                metallic_roughness_texture,
                "u_metallicRoughnessTexture",
                "METALLIC_ROUGHNESS",
                default_texture,
            );
        }
    }

    if metallic_roughness.metallic_factor != mc::MetallicRoughness::<G>::DEFAULT_METALLIC_FACTOR {
        shader_builder.add_uniform("float", "u_metallicFactor", ShaderDestination::FRAGMENT);
        let metallic_factor = metallic_roughness.metallic_factor;
        uniform_map
            .insert("u_metallicFactor".into(), Rc::new(move || UniformValue::Float(vec![metallic_factor as f32])));
        shader_builder.add_define("HAS_METALLIC_FACTOR", None, ShaderDestination::FRAGMENT);
    }

    if metallic_roughness.roughness_factor != mc::MetallicRoughness::<G>::DEFAULT_ROUGHNESS_FACTOR {
        shader_builder.add_uniform("float", "u_roughnessFactor", ShaderDestination::FRAGMENT);
        let roughness_factor = metallic_roughness.roughness_factor;
        uniform_map
            .insert("u_roughnessFactor".into(), Rc::new(move || UniformValue::Float(vec![roughness_factor as f32])));
        shader_builder.add_define("HAS_ROUGHNESS_FACTOR", None, ShaderDestination::FRAGMENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Cartesian3, Cartesian4, RuntimeError};
    use crate::renderer::GlValue;
    use crate::scene::model::{ModelRenderResources, ModelUniformState, NodeRenderResources};
    use crate::scene::AttributeType;
    use std::cell::RefCell;

    #[derive(Default)]
    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([8, 8])
        }
        fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            if method.starts_with("create") {
                Ok(GlValue::Object(1))
            } else {
                Ok(GlValue::Null)
            }
        }
    }

    fn make_primitive_with_normals<G: Gl + 'static>() -> mc::Primitive<G> {
        mc::Primitive {
            attributes: vec![
                mc::Attribute {
                    semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                    attribute_type: Some(AttributeType::Vec3),
                    count: Some(3),
                    min: Some(mc::MathValue::Cartesian3(Cartesian3::ZERO)),
                    max: Some(mc::MathValue::Cartesian3(Cartesian3::ONE)),
                    ..Default::default()
                },
                mc::Attribute {
                    semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Normal)),
                    attribute_type: Some(AttributeType::Vec3),
                    count: Some(3),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn processes_metallic_roughness_and_extensions() {
        let ctx = Context::new_webgl2(TestGl);
        let state = Rc::new(RefCell::new(ModelUniformState::default()));
        let model_res = ModelRenderResources::<TestGl>::new(state);
        let node_res = NodeRenderResources::new(&model_res, 0);
        let mut primitive = make_primitive_with_normals();
        primitive.material = Some(mc::Material {
            metallic_roughness: mc::MetallicRoughness {
                base_color_factor: Cartesian4::new(0.5, 0.6, 0.7, 0.8),
                metallic_factor: 0.2,
                roughness_factor: 0.4,
                base_color_texture: Some(mc::TextureReader {
                    tex_coord: 1.0,
                    transform: Some(Matrix3::from_scale(Cartesian3::new(2.0, 3.0, 1.0))),
                    ..Default::default()
                }),
                ..Default::default()
            },
            emissive_factor: Cartesian3::new(0.1, 0.2, 0.3),
            emissive_texture: Some(mc::TextureReader::default()),
            normal_texture: Some(mc::TextureReader { scale: Some(0.5), ..Default::default() }),
            specular: Some(mc::Specular {
                specular_factor: 0.5,
                specular_color_factor: Cartesian3::new(0.9, 0.8, 0.7),
                ..Default::default()
            }),
            anisotropy: Some(mc::Anisotropy {
                anisotropy_strength: 0.75,
                anisotropy_rotation: 0.5,
                ..Default::default()
            }),
            clearcoat: Some(mc::Clearcoat {
                clearcoat_factor: 0.8,
                clearcoat_roughness_factor: 0.3,
                ..Default::default()
            }),
            alpha_mode: AlphaMode::Blend,
            double_sided: true,
            point_diameter: Some(4.0),
            ..Default::default()
        });

        let mut prim_res = PrimitiveRenderResources::new(&node_res, 0, &primitive, None, None).unwrap();
        process(&mut prim_res, &primitive, &ctx, 2.0);

        assert_eq!(prim_res.lighting_options.lighting_model, LightingModel::Pbr);
        assert!(!prim_res.render_state_options.cull.enabled);
        assert_eq!(prim_res.alpha_options.borrow().pass, Some(Pass::Translucent));
        assert_eq!((prim_res.uniform_map["u_pointDiameter"])(), UniformValue::Float(vec![8.0]));
        let (_, fs) = prim_res.shader_builder.sources();
        assert!(fs.defines.iter().any(|d| d == "USE_METALLIC_ROUGHNESS"));
        assert!(fs.defines.iter().any(|d| d == "HAS_BASE_COLOR_TEXTURE"));
        assert!(fs.defines.iter().any(|d| d == "TEXCOORD_BASE_COLOR v_texCoord_1"));
        assert!(fs.defines.iter().any(|d| d == "HAS_BASE_COLOR_TEXTURE_TRANSFORM"));
        assert!(fs.defines.iter().any(|d| d == "HAS_NORMAL_TEXTURE_SCALE"));
        assert!(fs.defines.iter().any(|d| d == "USE_SPECULAR"));
        assert!(fs.defines.iter().any(|d| d == "USE_ANISOTROPY"));
        assert!(fs.defines.iter().any(|d| d == "USE_CLEARCOAT"));
        assert!(fs.defines.iter().any(|d| d == "HAS_DOUBLE_SIDED_MATERIAL"));
    }
}
