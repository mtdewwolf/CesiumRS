//! CesiumJS `PpeSource`: the source of a per-point error texture.
//!
//! As upstream, `SIGR`'s value is `"VARZ"`, so `from_str_value("VARZ")` is `VarZ`.

string_enum!(PpeSource {
    SigX = "SIGX" => "SIGX",
    SigY = "SIGY" => "SIGY",
    SigZ = "SIGZ" => "SIGZ",
    VarX = "VARX" => "VARX",
    VarY = "VARY" => "VARY",
    VarZ = "VARZ" => "VARZ",
    SigR = "VARZ" => "SIGR",
});
