KTX2 reference inputs are copied without modification from CesiumJS 1.139
(Apache-2.0; see the repository LICENSE and NOTICE):

- `Green*` and `Logo32x32_UASTC_Zstd.ktx2`: https://github.com/CesiumGS/cesium/tree/1.139/Specs/Data/Images
- `Cesium_Logo_*`: https://github.com/CesiumGS/cesium/tree/1.139/Apps/Sandcastle/images

They exercise uncompressed, ETC1S, UASTC, Zstd and mipmapped input. Synthetic
alpha slices (reusing pinned codebooks), cubemaps, half/float pixels and mipmaps are generated with `ktx-parse` by
`tools/cesium/fixtures/core-textures.mjs` and embedded in the JSON fixture.
