//! CesiumJS `VoxelRenderResources`: assembles the voxel fragment/vertex shader
//! source and the render-resource state shared by the draw commands.
use super::{
    ClippingPlaneCollection, ClippingPlaneHandle, MetadataComponentType, MetadataType, VoxelMetadataOrder,
    VoxelShape, VoxelShapeType,
};
use crate::{
    core::Cartesian3,
    renderer::{ShaderBuilder, ShaderDestination},
    shaders,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Inputs the primitive supplies to the render-resource constructor.
pub struct VoxelRenderResourcesOptions<'a> {
    pub custom_fragment_shader_text: Option<&'a str>,
    /// Custom-shader uniforms, in insertion order.
    pub custom_uniforms: &'a [(String, super::model::UniformType)],
    /// Primitive uniform-map names (`u_*`).
    pub uniform_names: &'a [String],
    pub clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    pub shape: &'a dyn VoxelShape,
    pub shape_type: VoxelShapeType,
    pub metadata_order: VoxelMetadataOrder,
    pub depth_test: bool,
    pub padding_before: Cartesian3,
    pub padding_after: Cartesian3,
    pub use_log_depth: bool,
    pub sample_count: usize,
}

/// The generated shader and state needed to build voxel commands.
pub struct VoxelRenderResources {
    pub shader_builder: ShaderBuilder,
    /// Uniform names from the primitive's map plus custom shader uniforms.
    pub uniform_names: Vec<String>,
    pub clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    pub clipping_planes_length: usize,
    pub render_bound_planes: Vec<ClippingPlaneHandle>,
    pub render_bound_planes_length: usize,
}

impl VoxelRenderResources {
    pub fn new(options: VoxelRenderResourcesOptions<'_>) -> Self {
        let mut shader_builder = ShaderBuilder::default();

        let mut uniform_names: Vec<String> = options.uniform_names.to_vec();
        for (name, _) in options.custom_uniforms {
            if !uniform_names.contains(name) {
                uniform_names.push(name.clone());
            }
        }
        for (name, uniform_type) in options.custom_uniforms {
            shader_builder.add_uniform(uniform_type.as_str(), name, ShaderDestination::FRAGMENT);
        }
        shader_builder.add_uniform(
            "sampler3D",
            "u_megatextureTextures[METADATA_COUNT]",
            ShaderDestination::FRAGMENT,
        );

        let clipping_planes_length = options
            .clipping_planes
            .as_ref()
            .filter(|planes| planes.borrow().enabled)
            .map_or(0, |planes| planes.borrow().len());
        let render_bound_planes = options.shape.render_bound_plane_handles();
        let render_bound_planes_length = render_bound_planes.len();

        shader_builder.add_vertex_lines(&[shaders::voxels::VOXEL_VS.source]);
        if options.metadata_order == VoxelMetadataOrder::YUp {
            shader_builder.add_define("Y_UP_METADATA_ORDER", None, ShaderDestination::FRAGMENT);
        }
        match options.shape_type {
            VoxelShapeType::Box => shader_builder.add_define("SHAPE_BOX", None, ShaderDestination::FRAGMENT),
            VoxelShapeType::Cylinder => {
                shader_builder.add_define("SHAPE_CYLINDER", None, ShaderDestination::FRAGMENT)
            }
            VoxelShapeType::Ellipsoid => {
                shader_builder.add_define("SHAPE_ELLIPSOID", None, ShaderDestination::FRAGMENT)
            }
        }

        let custom_text = options.custom_fragment_shader_text.unwrap_or("");
        shader_builder.add_fragment_lines(&[
            custom_text,
            "#line 0",
            shaders::voxels::OCTREE.source,
            shaders::voxels::VOXEL_UTILS.source,
            shaders::voxels::MEGATEXTURE.source,
            shaders::voxels::INTERSECTION_UTILS.source,
            shaders::voxels::INTERSECT_PLANE.source,
            shaders::voxels::INTERSECT_DEPTH.source,
        ]);

        if clipping_planes_length > 0 {
            shader_builder.add_define("CLIPPING_PLANES", None, ShaderDestination::FRAGMENT);
            shader_builder.add_define(
                "CLIPPING_PLANES_COUNT",
                Some(&clipping_planes_length.to_string()),
                ShaderDestination::FRAGMENT,
            );
            if options.clipping_planes.as_ref().is_some_and(|planes| planes.borrow().union_clipping_regions) {
                shader_builder.add_define("CLIPPING_PLANES_UNION", None, ShaderDestination::FRAGMENT);
            }
        }
        if options.depth_test {
            shader_builder.add_define("DEPTH_TEST", None, ShaderDestination::FRAGMENT);
        }

        match options.shape_type {
            VoxelShapeType::Box => shader_builder.add_fragment_lines(&[
                shaders::voxels::CONVERT_LOCAL_TO_BOX_UV.source,
                shaders::voxels::INTERSECT_BOX.source,
                shaders::voxels::INTERSECTION.source,
            ]),
            VoxelShapeType::Cylinder => shader_builder.add_fragment_lines(&[
                shaders::voxels::CONVERT_LOCAL_TO_CYLINDER_UV.source,
                shaders::voxels::INTERSECT_LONGITUDE.source,
                shaders::voxels::INTERSECT_CYLINDER.source,
                shaders::voxels::INTERSECTION.source,
            ]),
            VoxelShapeType::Ellipsoid => shader_builder.add_fragment_lines(&[
                shaders::voxels::CONVERT_LOCAL_TO_ELLIPSOID_UV.source,
                shaders::voxels::INTERSECT_LONGITUDE.source,
                shaders::voxels::INTERSECT_ELLIPSOID.source,
                shaders::voxels::INTERSECTION.source,
            ]),
        }
        shader_builder.add_fragment_lines(&[shaders::voxels::VOXEL_FS.source]);

        let shape_defines = options.shape.shader_defines();
        for (key, value) in shape_defines {
            // Upstream's true means `#define KEY` and undefined values are
            // omitted. Shape defines are strings; "true" is that case.
            if value == "true" {
                shader_builder.add_define(&key, None, ShaderDestination::FRAGMENT);
            } else {
                shader_builder.add_define(&key, Some(&value), ShaderDestination::FRAGMENT);
            }
        }

        let mut intersection_count = options.shape.shader_maximum_intersections_length();
        if clipping_planes_length > 0 {
            shader_builder.add_define(
                "CLIPPING_PLANES_INTERSECTION_INDEX",
                Some(&intersection_count.to_string()),
                ShaderDestination::FRAGMENT,
            );
            intersection_count += if clipping_planes_length == 1
                || !options
                    .clipping_planes
                    .as_ref()
                    .is_some_and(|planes| planes.borrow().union_clipping_regions)
            {
                1
            } else {
                2
            };
        }
        if options.depth_test {
            shader_builder.add_define(
                "DEPTH_INTERSECTION_INDEX",
                Some(&intersection_count.to_string()),
                ShaderDestination::FRAGMENT,
            );
            intersection_count += 1;
        }
        shader_builder.add_define(
            "INTERSECTION_COUNT",
            Some(&intersection_count.to_string()),
            ShaderDestination::FRAGMENT,
        );

        if options.padding_before != Cartesian3::ZERO || options.padding_after != Cartesian3::ZERO {
            shader_builder.add_define("PADDING", None, ShaderDestination::FRAGMENT);
        }
        if options.use_log_depth {
            shader_builder.add_define("LOG_DEPTH_READ_ONLY", None, ShaderDestination::FRAGMENT);
        }
        shader_builder.add_define(
            "SAMPLE_COUNT",
            Some(&options.sample_count.to_string()),
            ShaderDestination::FRAGMENT,
        );

        Self {
            shader_builder,
            uniform_names,
            clipping_planes: options.clipping_planes,
            clipping_planes_length,
            render_bound_planes,
            render_bound_planes_length,
        }
    }

    /// Shape uniforms merged into the primitive's map for validation and draws.
    pub fn shape_uniform_names(shape: &dyn VoxelShape) -> BTreeMap<String, crate::renderer::UniformValue> {
        shape.shader_uniforms()
    }

    /// The generated shader sources. Attribute locations are assigned by the
    /// caller because `VoxelVS` declares `position` itself.
    pub fn sources(&self) -> (crate::renderer::ShaderSource, crate::renderer::ShaderSource) {
        self.shader_builder.sources()
    }
}

/// Metadata types the generated texture reads support.
pub fn supported_metadata(metadata_type: MetadataType, component_type: MetadataComponentType) -> bool {
    matches!(
        metadata_type,
        MetadataType::Scalar | MetadataType::Vec2 | MetadataType::Vec3 | MetadataType::Vec4
    ) && matches!(
        component_type,
        MetadataComponentType::Uint8
            | MetadataComponentType::Float32
            | MetadataComponentType::Float64
    )
}
