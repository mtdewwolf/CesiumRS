//! Cesium `PostProcessStageLibrary`: factory functions for common post-process stages.
use crate::{
    core::{create_guid::create_guid, Color, Ellipsoid, RuntimeError},
    renderer::*,
    scene::{
        auto_exposure::AutoExposure,
        post_process_stage::{
            PostProcessStage, PostProcessStageOptions, PostProcessUniformValue,
        },
        post_process_stage_composite::{
            PostProcessStageComposite, PostProcessStageCompositeOptions, PostProcessStageItem,
        },
        PostProcessStageSampleMode,
    },
    shaders::{
        post_process_stages::{
            ACES_TONEMAPPING_STAGE, AMBIENT_OCCLUSION_GENERATE, AMBIENT_OCCLUSION_MODULATE,
            BLACK_AND_WHITE, BLOOM_COMPOSITE, BRIGHTNESS, CONTRAST_BIAS, DEPTH_OF_FIELD,
            DEPTH_VIEW, EDGE_DETECTION, FILMIC_TONEMAPPING, GAUSSIAN_BLUR1D, LENS_FLARE,
            MODIFIED_REINHARD_TONEMAPPING, NIGHT_VISION, PBR_NEUTRAL_TONEMAPPING,
            REINHARD_TONEMAPPING, SILHOUETTE,
        },
        root::FXAA3_11,
    },
};
use std::collections::HashMap;

/// Factory namespace for common post-process effects.
pub struct PostProcessStageLibrary;

impl PostProcessStageLibrary {
    /// Creates a post-process stage that applies a 2-pass Gaussian blur.
    pub fn create_blur_stage<G: Gl>(name: &str) -> Result<PostProcessStageComposite<G>, RuntimeError> {
        let blur_shader = format!("#define USE_STEP_SIZE\n{}", GAUSSIAN_BLUR1D.source);

        let mut x_opts = PostProcessStageOptions::new(blur_shader.clone());
        x_opts.name = Some(format!("{name}_x_direction"));
        x_opts.sample_mode = Some(PostProcessStageSampleMode::Linear);
        x_opts.uniforms.insert("delta".into(), 1.0_f32.into());
        x_opts.uniforms.insert("sigma".into(), 2.0_f32.into());
        x_opts.uniforms.insert("stepSize".into(), 1.0_f32.into());
        x_opts.uniforms.insert("direction".into(), 0.0_f32.into());
        let blur_x = PostProcessStage::new(x_opts)?;

        let mut y_opts = PostProcessStageOptions::new(blur_shader);
        y_opts.name = Some(format!("{name}_y_direction"));
        y_opts.sample_mode = Some(PostProcessStageSampleMode::Linear);
        y_opts.uniforms.insert("delta".into(), 1.0_f32.into());
        y_opts.uniforms.insert("sigma".into(), 2.0_f32.into());
        y_opts.uniforms.insert("stepSize".into(), 1.0_f32.into());
        y_opts.uniforms.insert("direction".into(), 1.0_f32.into());
        let blur_y = PostProcessStage::new(y_opts)?;

        let mut comp_opts = PostProcessStageCompositeOptions::new(vec![blur_x.into(), blur_y.into()]);
        comp_opts.name = Some(name.to_string());
        comp_opts.input_previous_stage_texture = Some(true);
        comp_opts.uniforms.insert("delta".into(), 1.0_f32.into());
        comp_opts.uniforms.insert("sigma".into(), 2.0_f32.into());
        comp_opts.uniforms.insert("stepSize".into(), 1.0_f32.into());

        PostProcessStageComposite::new(comp_opts)
    }

    /// Creates a post-process stage applying depth of field.
    pub fn create_depth_of_field_stage<G: Gl>() -> Result<PostProcessStageComposite<G>, RuntimeError> {
        let blur = Self::create_blur_stage("czm_depth_of_field_blur")?;

        let mut dof_opts = PostProcessStageOptions::new(DEPTH_OF_FIELD.source);
        dof_opts.name = Some("czm_depth_of_field_composite".to_string());
        dof_opts.uniforms.insert("focalDistance".into(), 5.0_f32.into());
        dof_opts.uniforms.insert("blurTexture".into(), "czm_depth_of_field_blur".into());
        let dof = PostProcessStage::new(dof_opts)?;

        let mut comp_opts = PostProcessStageCompositeOptions::new(vec![blur.into(), dof.into()]);
        comp_opts.name = Some("czm_depth_of_field".to_string());
        comp_opts.input_previous_stage_texture = Some(false);
        comp_opts.uniforms.insert("focalDistance".into(), 5.0_f32.into());
        comp_opts.uniforms.insert("delta".into(), 1.0_f32.into());
        comp_opts.uniforms.insert("sigma".into(), 2.0_f32.into());
        comp_opts.uniforms.insert("stepSize".into(), 1.0_f32.into());

        PostProcessStageComposite::new(comp_opts)
    }

    pub fn is_depth_of_field_supported<G: Gl>(context: &Context<G>) -> bool {
        context.capabilities().map(|c| c.depth_texture).unwrap_or(false)
    }

    /// Creates a post-process stage detecting edges.
    pub fn create_edge_detection_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let name = format!("czm_edge_detection_{}", create_guid());
        let mut opts = PostProcessStageOptions::new(EDGE_DETECTION.source);
        opts.name = Some(name);
        opts.uniforms.insert("length".into(), 0.25_f32.into());
        opts.uniforms.insert("color".into(), Color::BLACK.into());
        PostProcessStage::new(opts)
    }

    pub fn is_edge_detection_supported<G: Gl>(context: &Context<G>) -> bool {
        context.capabilities().map(|c| c.depth_texture).unwrap_or(false)
    }

    /// Creates a silhouette post-process stage.
    pub fn create_silhouette_stage<G: Gl>(
        edge_stages: Option<Vec<PostProcessStage<G>>>,
    ) -> Result<PostProcessStageComposite<G>, RuntimeError> {
        let edge_detection: PostProcessStageItem<G> = match edge_stages {
            None => Self::create_edge_detection_stage()?.into(),
            Some(stages) => {
                if stages.len() == 1 {
                    stages.into_iter().next().unwrap().into()
                } else {
                    let count = stages.len();
                    let stage_names: Vec<String> = stages.iter().map(|s| s.name().to_string()).collect();
                    let stage_items: Vec<PostProcessStageItem<G>> = stages.into_iter().map(Into::into).collect();

                    let mut mult_opts = PostProcessStageCompositeOptions::new(stage_items);
                    mult_opts.name = Some("czm_edge_detection_multiple".to_string());
                    mult_opts.input_previous_stage_texture = Some(false);
                    let edge_detection_mult = PostProcessStageComposite::new(mult_opts)?;

                    let mut fs_decl = String::new();
                    let mut fs_loop = String::new();
                    let mut composite_uniforms = HashMap::new();

                    for (i, stage_name) in stage_names.iter().enumerate() {
                        fs_decl.push_str(&format!("uniform sampler2D edgeTexture{i};\n"));
                        fs_loop.push_str(&format!(
                            "        vec4 edge{i} = texture(edgeTexture{i}, v_textureCoordinates);\n\
                             if (edge{i}.a > 0.0) {{\n\
                                 color = edge{i};\n\
                                 break;\n\
                             }}\n"
                        ));
                        composite_uniforms.insert(format!("edgeTexture{i}"), PostProcessUniformValue::StageName(stage_name.clone()));
                    }

                    let combine_fs = format!(
                        "{fs_decl}\
in vec2 v_textureCoordinates;\n\
void main() {{\n\
    vec4 color = vec4(0.0);\n\
    for (int i = 0; i < {count}; ++i) {{\n\
{fs_loop}\
    }}\n\
    out_FragColor = color;\n\
}}\n"
                    );

                    let mut combine_opts = PostProcessStageOptions::new(combine_fs);
                    combine_opts.name = Some("czm_edge_detection_combine".to_string());
                    combine_opts.uniforms = composite_uniforms;
                    let edge_combine = PostProcessStage::new(combine_opts)?;

                    let mut comp_opts = PostProcessStageCompositeOptions::new(vec![
                        edge_detection_mult.into(),
                        edge_combine.into(),
                    ]);
                    comp_opts.name = Some("czm_edge_detection_composite".to_string());
                    PostProcessStageComposite::new(comp_opts)?.into()
                }
            }
        };

        let edge_name = edge_detection.name().to_string();

        let mut sil_opts = PostProcessStageOptions::new(SILHOUETTE.source);
        sil_opts.name = Some("czm_silhouette_color_edges".to_string());
        sil_opts.uniforms.insert("silhouetteTexture".into(), edge_name.into());
        let silhouette_process = PostProcessStage::new(sil_opts)?;

        let mut composite_opts = PostProcessStageCompositeOptions::new(vec![
            edge_detection,
            silhouette_process.into(),
        ]);
        composite_opts.name = Some("czm_silhouette".to_string());
        composite_opts.input_previous_stage_texture = Some(false);
        composite_opts.uniforms.insert("length".into(), 0.5_f32.into());
        composite_opts.uniforms.insert("color".into(), Color::BLACK.into());

        PostProcessStageComposite::new(composite_opts)
    }

    pub fn is_silhouette_supported<G: Gl>(context: &Context<G>) -> bool {
        context.capabilities().map(|c| c.depth_texture).unwrap_or(false)
    }

    /// Creates a bloom post-process stage.
    pub fn create_bloom_stage<G: Gl>() -> Result<PostProcessStageComposite<G>, RuntimeError> {
        let mut cb_opts = PostProcessStageOptions::new(CONTRAST_BIAS.source);
        cb_opts.name = Some("czm_bloom_contrast_bias".to_string());
        cb_opts.uniforms.insert("contrast".into(), 128.0_f32.into());
        cb_opts.uniforms.insert("brightness".into(), (-0.3_f32).into());
        let contrast_bias = PostProcessStage::new(cb_opts)?;

        let blur = Self::create_blur_stage("czm_bloom_blur")?;

        let mut gen_opts = PostProcessStageCompositeOptions::new(vec![contrast_bias.into(), blur.into()]);
        gen_opts.name = Some("czm_bloom_contrast_bias_blur".to_string());
        gen_opts.input_previous_stage_texture = Some(true);
        let generate_composite = PostProcessStageComposite::new(gen_opts)?;

        let mut comp_stage_opts = PostProcessStageOptions::new(BLOOM_COMPOSITE.source);
        comp_stage_opts.name = Some("czm_bloom_generate_composite".to_string());
        comp_stage_opts.uniforms.insert("glowOnly".into(), false.into());
        comp_stage_opts.uniforms.insert("bloomTexture".into(), "czm_bloom_contrast_bias_blur".into());
        let bloom_composite = PostProcessStage::new(comp_stage_opts)?;

        let mut bloom_opts = PostProcessStageCompositeOptions::new(vec![
            generate_composite.into(),
            bloom_composite.into(),
        ]);
        bloom_opts.name = Some("czm_bloom".to_string());
        bloom_opts.input_previous_stage_texture = Some(false);
        bloom_opts.uniforms.insert("glowOnly".into(), false.into());
        bloom_opts.uniforms.insert("contrast".into(), 128.0_f32.into());
        bloom_opts.uniforms.insert("brightness".into(), (-0.3_f32).into());
        bloom_opts.uniforms.insert("delta".into(), 1.0_f32.into());
        bloom_opts.uniforms.insert("sigma".into(), 2.0_f32.into());
        bloom_opts.uniforms.insert("stepSize".into(), 1.0_f32.into());

        PostProcessStageComposite::new(bloom_opts)
    }

    /// Creates an ambient occlusion post-process stage (HBAO).
    pub fn create_ambient_occlusion_stage<G: Gl>() -> Result<PostProcessStageComposite<G>, RuntimeError> {
        let mut gen_opts = PostProcessStageOptions::new(AMBIENT_OCCLUSION_GENERATE.source);
        gen_opts.name = Some("czm_ambient_occlusion_generate".to_string());
        gen_opts.uniforms.insert("intensity".into(), 3.0_f32.into());
        gen_opts.uniforms.insert("bias".into(), 0.1_f32.into());
        gen_opts.uniforms.insert("lengthCap".into(), 0.26_f32.into());
        gen_opts.uniforms.insert("directionCount".into(), 8_i32.into());
        gen_opts.uniforms.insert("stepCount".into(), 32_i32.into());
        let generate = PostProcessStage::new(gen_opts)?;

        let mut mod_opts = PostProcessStageOptions::new(AMBIENT_OCCLUSION_MODULATE.source);
        mod_opts.name = Some("czm_ambient_occlusion_composite".to_string());
        mod_opts.uniforms.insert("ambientOcclusionOnly".into(), false.into());
        mod_opts.uniforms.insert("ambientOcclusionTexture".into(), "czm_ambient_occlusion_generate".into());
        let modulate = PostProcessStage::new(mod_opts)?;

        let mut comp_opts = PostProcessStageCompositeOptions::new(vec![generate.into(), modulate.into()]);
        comp_opts.name = Some("czm_ambient_occlusion".to_string());
        comp_opts.input_previous_stage_texture = Some(false);
        comp_opts.uniforms.insert("intensity".into(), 3.0_f32.into());
        comp_opts.uniforms.insert("bias".into(), 0.1_f32.into());
        comp_opts.uniforms.insert("lengthCap".into(), 0.26_f32.into());
        comp_opts.uniforms.insert("directionCount".into(), 8_i32.into());
        comp_opts.uniforms.insert("stepCount".into(), 32_i32.into());
        comp_opts.uniforms.insert("ambientOcclusionOnly".into(), false.into());

        PostProcessStageComposite::new(comp_opts)
    }

    pub fn is_ambient_occlusion_supported<G: Gl>(context: &Context<G>) -> bool {
        context.capabilities().map(|c| c.depth_texture).unwrap_or(false)
    }

    /// Creates an FXAA (Fast Approximate Anti-aliasing) stage.
    pub fn create_fxaa_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let fxaa_shader = format!(
            "#define FXAA_QUALITY_PRESET 39\n{}\n{}",
            FXAA3_11.source,
            crate::shaders::post_process_stages::FXAA.source
        );
        let mut opts = PostProcessStageOptions::new(fxaa_shader);
        opts.name = Some("czm_FXAA".to_string());
        opts.sample_mode = Some(PostProcessStageSampleMode::Linear);
        PostProcessStage::new(opts)
    }

    /// Creates an ACES tone mapping stage.
    pub fn create_aces_tonemapping_stage<G: Gl>(use_auto_exposure: bool) -> Result<PostProcessStage<G>, RuntimeError> {
        let prefix = if use_auto_exposure { "#define AUTO_EXPOSURE\n" } else { "" };
        let mut opts = PostProcessStageOptions::new(format!("{prefix}{}", ACES_TONEMAPPING_STAGE.source));
        opts.name = Some("czm_aces".to_string());
        opts.uniforms.insert("exposure".into(), 1.0_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates a filmic tone mapping stage.
    pub fn create_filmic_tonemapping_stage<G: Gl>(use_auto_exposure: bool) -> Result<PostProcessStage<G>, RuntimeError> {
        let prefix = if use_auto_exposure { "#define AUTO_EXPOSURE\n" } else { "" };
        let mut opts = PostProcessStageOptions::new(format!("{prefix}{}", FILMIC_TONEMAPPING.source));
        opts.name = Some("czm_filmic".to_string());
        opts.uniforms.insert("exposure".into(), 1.0_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates a PBR Neutral tone mapping stage.
    pub fn create_pbr_neutral_tonemapping_stage<G: Gl>(use_auto_exposure: bool) -> Result<PostProcessStage<G>, RuntimeError> {
        let prefix = if use_auto_exposure { "#define AUTO_EXPOSURE\n" } else { "" };
        let mut opts = PostProcessStageOptions::new(format!("{prefix}{}", PBR_NEUTRAL_TONEMAPPING.source));
        opts.name = Some("czm_pbr_neutral".to_string());
        opts.uniforms.insert("exposure".into(), 1.0_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates a Reinhard tone mapping stage.
    pub fn create_reinhard_tonemapping_stage<G: Gl>(use_auto_exposure: bool) -> Result<PostProcessStage<G>, RuntimeError> {
        let prefix = if use_auto_exposure { "#define AUTO_EXPOSURE\n" } else { "" };
        let mut opts = PostProcessStageOptions::new(format!("{prefix}{}", REINHARD_TONEMAPPING.source));
        opts.name = Some("czm_reinhard".to_string());
        opts.uniforms.insert("exposure".into(), 1.0_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates a modified Reinhard tone mapping stage.
    pub fn create_modified_reinhard_tonemapping_stage<G: Gl>(
        use_auto_exposure: bool,
    ) -> Result<PostProcessStage<G>, RuntimeError> {
        let prefix = if use_auto_exposure { "#define AUTO_EXPOSURE\n" } else { "" };
        let mut opts = PostProcessStageOptions::new(format!("{prefix}{}", MODIFIED_REINHARD_TONEMAPPING.source));
        opts.name = Some("czm_modified_reinhard".to_string());
        opts.uniforms.insert("white".into(), Color::WHITE.into());
        opts.uniforms.insert("exposure".into(), 1.0_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates an auto-exposure stage.
    pub fn create_auto_exposure_stage<G: Gl>() -> AutoExposure<G> {
        AutoExposure::new()
    }

    /// Creates a black-and-white gradation stage.
    pub fn create_black_and_white_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let mut opts = PostProcessStageOptions::new(BLACK_AND_WHITE.source);
        opts.name = Some("czm_black_and_white".to_string());
        opts.uniforms.insert("gradations".into(), 5.0_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates a brightness/saturation stage.
    pub fn create_brightness_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let mut opts = PostProcessStageOptions::new(BRIGHTNESS.source);
        opts.name = Some("czm_brightness".to_string());
        opts.uniforms.insert("brightness".into(), 0.5_f32.into());
        PostProcessStage::new(opts)
    }

    /// Creates a night vision stage.
    pub fn create_night_vision_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let mut opts = PostProcessStageOptions::new(NIGHT_VISION.source);
        opts.name = Some("czm_night_vision".to_string());
        PostProcessStage::new(opts)
    }

    /// Creates a depth view inspection stage.
    pub fn create_depth_view_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let mut opts = PostProcessStageOptions::new(DEPTH_VIEW.source);
        opts.name = Some("czm_depth_view".to_string());
        PostProcessStage::new(opts)
    }

    /// Creates a lens flare effect stage.
    pub fn create_lens_flare_stage<G: Gl>() -> Result<PostProcessStage<G>, RuntimeError> {
        let mut opts = PostProcessStageOptions::new(LENS_FLARE.source);
        opts.name = Some("czm_lens_flare".to_string());
        opts.uniforms.insert("intensity".into(), 2.0_f32.into());
        opts.uniforms.insert("distortion".into(), 10.0_f32.into());
        opts.uniforms.insert("ghostDispersal".into(), 0.4_f32.into());
        opts.uniforms.insert("haloWidth".into(), 0.4_f32.into());
        opts.uniforms.insert("dirtAmount".into(), 0.4_f32.into());
        opts.uniforms.insert("earthRadius".into(), (Ellipsoid::WGS84.maximum_radius as f32).into());
        PostProcessStage::new(opts)
    }
}
