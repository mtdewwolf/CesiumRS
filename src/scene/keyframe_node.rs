//! CesiumJS `KeyframeNode`: one keyframe of a voxel spatial node, with its
//! load state, content and megatexture slot.

/// `KeyframeNode.LoadState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyframeLoadState {
    /// No data, dormant.
    Unloaded = 0,
    /// Waiting on data from the provider.
    Receiving = 1,
    /// Data received and being processed for rendering.
    Processing = 2,
    Loaded = 3,
    /// The provider failed to supply data.
    Failed = 4,
    /// The provider has no data for this keyframe.
    Unavailable = 5,
}

/// `S` is the voxel spatial node and `C` the content, which is released on
/// unload.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyframeNode<S, C> {
    pub spatial_node: Option<S>,
    pub keyframe: usize,
    pub state: KeyframeLoadState,
    pub content: Option<C>,
    /// -1 when the keyframe has no megatexture slot.
    pub megatexture_index: i64,
    pub priority: f64,
    pub high_priority_frame_number: i64,
}

impl<S, C> KeyframeNode<S, C> {
    pub fn new(spatial_node: S, keyframe: usize) -> Self {
        Self {
            spatial_node: Some(spatial_node),
            keyframe,
            state: KeyframeLoadState::Unloaded,
            content: None,
            megatexture_index: -1,
            priority: -f64::MAX,
            high_priority_frame_number: -1,
        }
    }

    /// Releases the content and detaches the node from its spatial node.
    pub fn unload(&mut self) {
        self.content = None;
        self.spatial_node = None;
        self.state = KeyframeLoadState::Unloaded;
        self.megatexture_index = -1;
        self.priority = -f64::MAX;
        self.high_priority_frame_number = -1;
    }

    /// Orders by priority, lowest first.
    pub fn priority_comparator(a: &Self, b: &Self) -> f64 {
        a.priority - b.priority
    }

    /// Orders by keyframe, for binary search.
    pub fn search_comparator(a: &Self, b: &Self) -> f64 {
        a.keyframe as f64 - b.keyframe as f64
    }
}
