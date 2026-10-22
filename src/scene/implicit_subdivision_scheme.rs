//! CesiumJS `ImplicitSubdivisionScheme`: how an implicit tileset subdivides.

string_enum!(ImplicitSubdivisionScheme {
    Quadtree = "QUADTREE" => "QUADTREE",
    Octree = "OCTREE" => "OCTREE",
});

impl ImplicitSubdivisionScheme {
    /// Children per tile: 4 for a quadtree, 8 for an octree.
    pub fn get_branching_factor(self) -> u32 {
        match self {
            Self::Octree => 8,
            Self::Quadtree => 4,
        }
    }
}
