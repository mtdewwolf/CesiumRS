//! CesiumJS `Scene/Model/B3dmLoader`: loads a Batched 3D Model (`.b3dm`) tile's
//! embedded glTF and transcodes its feature/batch tables into `ModelComponents`.

use crate::core::{Cartesian3, ComponentDatatype, Matrix4, Resource, RuntimeError};
use crate::renderer::{Context, Gl};
use crate::scene::b3dm_parser::{self, TileFormatWarning};
use crate::scene::cesium_3d_tile_feature_table::Cesium3DTileFeatureTable;
use crate::scene::gltf_loader::{GltfLoader, GltfLoaderOptions};
use crate::scene::gltf_texture_loader::GltfTextureOptions;
use crate::scene::metadata_table_property::ByteView;
use crate::scene::model_components::{self as mc, AttributeSemantic};
use crate::scene::parse_batch_table::{parse_batch_table, ParseBatchTableOptions};
use crate::scene::resource_loader::{self, ResourceLoadFuture, ResourceLoader};
use crate::scene::structural_metadata::{StructuralMetadataOptions, StructuralMetadataWarning};
use crate::scene::{
    Axis, FrameResource, FrameState, GltfJsonInput, MetadataClass, ModelImageHost, PropertyTable, PropertyTableOptions,
    ResourceCache, ResourceLoaderState, StructuralMetadata, VertexAttributeSemantic,
};
use futures_util::FutureExt;
use serde_json::Value;
use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct B3dmLoaderOptions {
    pub release_gltf_json: bool,
    pub asynchronous: bool,
    pub incrementally_load_textures: bool,
    pub up_axis: Axis,
    pub forward_axis: Axis,
    pub load_attributes_as_typed_array: bool,
    pub load_attributes_for_2d: bool,
    pub enable_pick: bool,
    pub load_indices_for_wireframe: bool,
    pub load_primitive_outline: bool,
    pub load_for_classification: bool,
    pub texture_options: GltfTextureOptions,
}

impl Default for B3dmLoaderOptions {
    fn default() -> Self {
        Self {
            release_gltf_json: false,
            asynchronous: true,
            incrementally_load_textures: true,
            up_axis: Axis::Y,
            forward_axis: Axis::X,
            load_attributes_as_typed_array: false,
            load_attributes_for_2d: false,
            enable_pick: false,
            load_indices_for_wireframe: false,
            load_primitive_outline: true,
            load_for_classification: false,
            texture_options: GltfTextureOptions::default(),
        }
    }
}

type ChildLoader<G, H> = Rc<RefCell<Option<Rc<GltfLoader<G, H>>>>>;

struct ParsedState {
    batch_length: usize,
    batch_table_json: Option<Value>,
    batch_table_binary: Option<Vec<u8>>,
}

pub struct B3dmLoader<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    cache: ResourceCache,
    resource: Resource,
    base_resource: Resource,
    array_buffer: Rc<RefCell<Option<Arc<Vec<u8>>>>>,
    byte_offset: usize,
    context: Context<G>,
    host: Rc<H>,
    options: B3dmLoaderOptions,
    state: Rc<Cell<ResourceLoaderState>>,
    destroyed: Rc<Cell<bool>>,
    transform: Cell<Matrix4>,
    batch_length: Cell<usize>,
    parsed: Rc<RefCell<Option<ParsedState>>>,
    gltf_loader: ChildLoader<G, H>,
    components: RefCell<Option<mc::Components<G>>>,
    promise: RefCell<Option<ResourceLoadFuture>>,
    warnings: RefCell<Vec<TileFormatWarning>>,
    metadata_warnings: RefCell<Vec<StructuralMetadataWarning>>,
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> B3dmLoader<G, H>
where
    H::Image: 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cache: ResourceCache,
        resource: Resource,
        base_resource: Option<Resource>,
        array_buffer: Arc<Vec<u8>>,
        byte_offset: Option<usize>,
        context: Context<G>,
        host: Rc<H>,
        options: B3dmLoaderOptions,
    ) -> Self {
        let base_resource = base_resource.unwrap_or_else(|| resource.clone());
        let byte_offset = byte_offset.unwrap_or(0);
        Self {
            cache,
            resource,
            base_resource,
            array_buffer: Rc::new(RefCell::new(Some(array_buffer))),
            byte_offset,
            context,
            host,
            options,
            state: Rc::new(Cell::new(ResourceLoaderState::Unloaded)),
            destroyed: Rc::new(Cell::new(false)),
            transform: Cell::new(Matrix4::IDENTITY),
            batch_length: Cell::new(0),
            parsed: Rc::new(RefCell::new(None)),
            gltf_loader: Rc::new(RefCell::new(None)),
            components: RefCell::new(None),
            promise: RefCell::new(None),
            warnings: RefCell::new(Vec::new()),
            metadata_warnings: RefCell::new(Vec::new()),
        }
    }

    pub fn components(&self) -> Option<Ref<'_, mc::Components<G>>> {
        Ref::filter_map(self.components.borrow(), Option::as_ref).ok()
    }

    pub fn textures_loaded(&self) -> bool {
        self.gltf_loader.borrow().as_ref().is_some_and(|l| l.textures_loaded())
    }

    pub fn incrementally_load_textures(&self) -> bool {
        self.options.incrementally_load_textures
    }

    pub fn transform(&self) -> Matrix4 {
        self.transform.get()
    }

    pub fn batch_length(&self) -> usize {
        self.batch_length.get()
    }

    pub fn warnings(&self) -> Vec<TileFormatWarning> {
        self.warnings.borrow().clone()
    }

    pub fn metadata_warnings(&self) -> Vec<StructuralMetadataWarning> {
        self.metadata_warnings.borrow().clone()
    }

    pub fn is_unloaded(&self) -> bool {
        self.state.get() == ResourceLoaderState::Unloaded
    }

    fn create_structural_metadata(&self, components: &mut mc::Components<G>) -> Result<(), RuntimeError> {
        let Some(parsed) = self.parsed.borrow_mut().take() else {
            return Ok(());
        };
        let batch_length = parsed.batch_length;
        if batch_length == 0 {
            return Ok(());
        }
        if let Some(scene) = &components.scene {
            let root_nodes = scene.nodes.clone();
            for node_index in root_nodes {
                Self::process_node(&mut components.nodes, node_index);
            }
        }
        let structural_metadata: StructuralMetadata<G> = if let Some(batch_table_json) = &parsed.batch_table_json {
            let mut warnings = Vec::new();
            let binary_body = parsed.batch_table_binary.as_deref().map(|buffer| ByteView { buffer, byte_offset: 0 });
            let metadata: StructuralMetadata<G> = parse_batch_table(
                &ParseBatchTableOptions {
                    count: batch_length,
                    batch_table: batch_table_json,
                    binary_body,
                    parse_as_property_attributes: false,
                    typed_arrays: None,
                },
                None,
                &mut warnings,
            )
            .map_err(|e| RuntimeError::new(e.to_string()))?;
            *self.metadata_warnings.borrow_mut() = warnings;
            metadata
        } else {
            let empty_property_table = PropertyTable::new(PropertyTableOptions {
                name: Some(MetadataClass::BATCH_TABLE_CLASS_NAME.into()),
                count: batch_length,
                ..Default::default()
            });
            StructuralMetadata::new(StructuralMetadataOptions {
                property_tables: Some(vec![empty_property_table]),
                ..Default::default()
            })
        };
        components.structural_metadata = Some(FrameResource::new(Rc::new(structural_metadata)));
        Ok(())
    }

    fn process_node(nodes: &mut [mc::Node<G>], node_index: usize) {
        let Some(node) = nodes.get_mut(node_index) else {
            return;
        };
        let children = node.children.clone();
        for primitive in &mut node.primitives {
            if let Some(attr) = primitive
                .attributes
                .iter_mut()
                .find(|a| a.semantic == Some(AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)))
            {
                attr.set_index = Some(0);
                primitive.feature_ids.push(mc::FeatureId::Attribute(mc::FeatureIdAttribute {
                    feature_count: None,
                    null_feature_id: None,
                    property_table_id: Some(0),
                    set_index: Some(0),
                    label: None,
                    positional_label: Some("featureId_0".into()),
                }));
            }
        }
        for child_index in children {
            Self::process_node(nodes, child_index);
        }
    }
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ResourceLoader for B3dmLoader<G, H>
where
    H::Image: 'static,
{
    fn cache_key(&self) -> Option<&str> {
        None
    }

    fn load(&self) -> ResourceLoadFuture {
        if self.is_destroyed() {
            return async { Ok(false) }.boxed_local().shared();
        }
        if let Some(promise) = self.promise.borrow().as_ref() {
            return promise.clone();
        }
        self.state.set(ResourceLoaderState::Loading);

        let parse_result = (|| -> Result<Rc<GltfLoader<G, H>>, RuntimeError> {
            let array_buffer =
                self.array_buffer.borrow().clone().ok_or_else(|| RuntimeError::new("b3dm arrayBuffer was unloaded"))?;
            let b3dm = b3dm_parser::parse(&array_buffer, self.byte_offset)?;
            let mut batch_length = b3dm.batch_length.unwrap_or(0) as usize;
            let mut feature_table = Cesium3DTileFeatureTable::new(b3dm.feature_table_json, b3dm.feature_table_binary);
            if batch_length == 0 {
                if let Some(prop) = feature_table
                    .get_global_property("BATCH_LENGTH", None, None)
                    .map_err(|e| RuntimeError::new(e.to_string()))?
                {
                    let n = prop.to_number();
                    if n.is_finite() && n >= 0.0 {
                        batch_length = n as usize;
                    }
                }
            }
            feature_table.features_length = batch_length as f64;
            if let Some(rtc) = feature_table
                .get_global_property("RTC_CENTER", Some(ComponentDatatype::Float), Some(3))
                .map_err(|e| RuntimeError::new(e.to_string()))?
            {
                let x = rtc.element(0).unwrap_or(0.0);
                let y = rtc.element(1).unwrap_or(0.0);
                let z = rtc.element(2).unwrap_or(0.0);
                self.transform.set(Matrix4::from_translation(Cartesian3::new(x, y, z)));
            }
            self.batch_length.set(batch_length);
            *self.warnings.borrow_mut() = b3dm.warnings;
            *self.parsed.borrow_mut() = Some(ParsedState {
                batch_length,
                batch_table_json: b3dm.batch_table_json,
                batch_table_binary: b3dm.batch_table_binary,
            });
            let gltf_options = GltfLoaderOptions {
                release_gltf_json: self.options.release_gltf_json,
                asynchronous: self.options.asynchronous,
                incrementally_load_textures: self.options.incrementally_load_textures,
                up_axis: self.options.up_axis,
                forward_axis: self.options.forward_axis,
                load_attributes_as_typed_array: self.options.load_attributes_as_typed_array,
                load_attributes_for_2d: self.options.load_attributes_for_2d,
                enable_pick: self.options.enable_pick,
                load_indices_for_wireframe: self.options.load_indices_for_wireframe,
                load_primitive_outline: self.options.load_primitive_outline,
                load_for_classification: self.options.load_for_classification,
                rename_batch_id_semantic: true,
                texture_options: self.options.texture_options,
            };
            let gltf_loader = Rc::new(GltfLoader::new(
                self.cache.clone(),
                self.resource.clone(),
                Some(self.base_resource.clone()),
                Some(GltfJsonInput::Bytes(b3dm.gltf)),
                self.context.clone(),
                self.host.clone(),
                gltf_options,
            ));
            *self.gltf_loader.borrow_mut() = Some(gltf_loader.clone());
            Ok(gltf_loader)
        })();

        let state = match parse_result {
            Ok(gltf_loader) => {
                let child_future = gltf_loader.load();
                let gltf_ref = self.gltf_loader.clone();
                let parsed_ref = self.parsed.clone();
                let array_buf_ref = self.array_buffer.clone();
                let state_cell = self.state.clone();
                let destroyed_cell = self.destroyed.clone();
                async move {
                    match child_future.await {
                        Ok(loaded) => {
                            if !loaded || destroyed_cell.get() || state_cell.get() == ResourceLoaderState::Unloaded {
                                return Ok(false);
                            }
                            state_cell.set(ResourceLoaderState::Loaded);
                            Ok(true)
                        }
                        Err(e) => {
                            if destroyed_cell.get() || state_cell.get() == ResourceLoaderState::Unloaded {
                                return Ok(false);
                            }
                            if !destroyed_cell.get() {
                                if let Some(loader) = gltf_ref.borrow_mut().take() {
                                    loader.destroy();
                                }
                                parsed_ref.borrow_mut().take();
                                array_buf_ref.borrow_mut().take();
                                state_cell.set(ResourceLoaderState::Failed);
                            }
                            Err(resource_loader::loader_error("Failed to load b3dm", Some(&e)))
                        }
                    }
                }
                .boxed_local()
                .shared()
            }
            Err(e) => {
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                let err = resource_loader::loader_error("Failed to load b3dm", Some(&e));
                async move { Err(err) }.boxed_local().shared()
            }
        };
        *self.promise.borrow_mut() = Some(state.clone());
        state
    }

    fn state(&self) -> ResourceLoaderState {
        self.state.get()
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }

    fn unload(&self) {
        if let Some(loader) = self.gltf_loader.borrow_mut().take() {
            loader.destroy();
        }
        self.components.borrow_mut().take();
        self.parsed.borrow_mut().take();
        self.array_buffer.borrow_mut().take();
        self.promise.borrow_mut().take();
        self.state.set(ResourceLoaderState::Unloaded);
    }

    fn destroy(&self) {
        self.unload();
        self.destroyed.set(true);
    }

    fn process(&self, frame: &FrameState) -> Result<bool, RuntimeError> {
        if self.state.get() == ResourceLoaderState::Ready {
            // Geometry may become ready before textures. Continue driving the
            // child and refresh the component snapshot as textures arrive.
            if let Some(loader) = self.gltf_loader.borrow().clone() {
                if loader.textures_loaded() {
                    return Ok(true);
                }
                if let Err(error) = loader.process(frame) {
                    return Err(resource_loader::loader_error("Failed to load b3dm", Some(&error)));
                }
                if let Some(components) = loader.components() {
                    *self.components.borrow_mut() = Some(components.clone());
                }
            }
            return Ok(true);
        }
        if self.state.get() != ResourceLoaderState::Loaded && self.state.get() != ResourceLoaderState::Processing {
            return Ok(false);
        }
        self.state.set(ResourceLoaderState::Processing);
        let Some(gltf_loader) = self.gltf_loader.borrow().clone() else {
            return Ok(false);
        };
        let ready = match gltf_loader.process(frame) {
            Ok(ready) => ready,
            Err(e) => {
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                return Err(resource_loader::loader_error("Failed to load b3dm", Some(&e)));
            }
        };
        if !ready {
            return Ok(false);
        }
        if let Some(mut components) = gltf_loader.components_mut() {
            components.transform = self.transform.get().multiply(&components.transform);
            if let Err(e) = self.create_structural_metadata(&mut components) {
                drop(components);
                self.unload();
                self.state.set(ResourceLoaderState::Failed);
                return Err(resource_loader::loader_error("Failed to load b3dm", Some(&e)));
            }
            *self.components.borrow_mut() = Some(components.clone());
        }
        self.array_buffer.borrow_mut().take();
        self.parsed.borrow_mut().take();
        self.state.set(ResourceLoaderState::Ready);
        Ok(true)
    }
}
