//! CesiumJS `PrimitiveState`: the lifecycle state of a `Primitive`.

// The variants are Cesium's `_state` values: nothing done, worker creation,
// worker-created geometry, worker combination, combined geometry, uploaded
// draw commands, and failure.
numeric_enum!(PrimitiveState: u32 {
    Ready = 0 => "READY",
    Creating = 1 => "CREATING",
    Created = 2 => "CREATED",
    Combining = 3 => "COMBINING",
    Combined = 4 => "COMBINED",
    Complete = 5 => "COMPLETE",
    Failed = 6 => "FAILED",
});

impl PrimitiveState {
    /// Whether the state only exists for asynchronous primitives.
    pub fn is_asynchronous(self) -> bool {
        matches!(self, Self::Creating | Self::Created | Self::Combining)
    }
    /// Whether geometry creation is finished (successfully or not).
    pub fn is_terminal_creation(self) -> bool {
        matches!(self, Self::Combined | Self::Complete | Self::Failed)
    }
}
