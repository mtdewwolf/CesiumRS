//! CesiumJS `StorageType`: how NGA_gpm_local stores its per-point error data.

string_enum!(StorageType {
    Direct = "Direct" => "Direct",
    Indirect = "Indirect" => "Indirect",
});
