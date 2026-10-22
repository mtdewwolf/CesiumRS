# Local changes to draco-core 2.3.2

This directory is a vendored copy of `draco-core` 2.3.2
(<https://github.com/Filyus/draco-rust>, Apache-2.0), taken from the crates.io
release. `README.md` and `CHANGELOG.md` are upstream's. `Cargo.toml` is
rewritten for vendoring: `publish = false`, no tests, examples or benches,
and a decoder-only default feature set.

## Skip attribute transforms

`workers::decode_draco` needs CesiumJS's `dequantizeInShader` path. That path
calls C++ Draco's `Decoder::SetSkipAttributeTransform`, which keeps an
attribute's quantized portable values and its transform parameters instead of
inverse-transforming them. Upstream 2.3.2 always inverse-transforms, so this
copy adds:

- `src/skip_transform.rs`: `SkipAttributeTransforms`, a set of
  `GeometryAttributeType`s, and helpers that keep a portable attribute together
  with its `AttributeTransformData`.
- `PointAttribute::keep_portable` (`src/geometry_attribute.rs`): replaces the
  attribute's values with the portable ones, keeping its type, unique id and
  point mapping and attaching the transform data.
- `MeshDecoder::set_skip_attribute_transform` and
  `PointCloudDecoder::set_skip_attribute_transform(s)`. A skipped attribute
  keeps one of the following:
  - a sequential quantized attribute: Int32 values plus quantization data;
  - an octahedral normal: 2-component Int32 values plus octahedron data;
  - an integer-encoded attribute: Int32 values with no transform data;
  - a KD-tree float attribute: Uint32 values plus quantization data.

  Generic attributes are unaffected, as in C++ Draco. The point-cloud decoder
  decodes sequentially whenever the skip set is non-empty.

With an empty skip set, the decoders behave exactly as upstream.

## Encoder removed

The encoder-only modules were deleted, along with their `mod` lines in
`src/lib.rs`:

direct_bit_encoder, encoder_buffer, encoder_options, kd_tree_attributes_encoder,
keyframe_animation_encoder, mesh_edgebreaker_encoder,
mesh_edgebreaker_traversal_predictive_encoder,
mesh_edgebreaker_traversal_valence_encoder, mesh_encoder, point_cloud_encoder,
point_order, prediction_scheme_normal_octahedron_canonicalized_encoding_transform,
rans_bit_encoder, rans_symbol_encoder, sequential_attribute_encoder,
sequential_integer_attribute_encoder, sequential_normal_attribute_encoder,
shannon_entropy.

The encode features stay declared so the remaining `cfg`s stay known, but
enabling `encoder` is a `compile_error!`.
