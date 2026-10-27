//! CesiumJS `createWorldImageryAsync`: ion's default global base imagery.
//!
//! Pinned upstream exposes only the async factory. `createWorldImagery` was
//! removed from CesiumJS, so this port offers the executable request
//! descriptor [`WorldImageryRequest`] instead of pretending a synchronous
//! network call exists.

use super::ion_imagery_provider::{
    create_world_imagery_async as create_async, IonImageryProvider, IonImageryProviderOptions,
};
use super::ImageryLoader;
use super::IonWorldImageryStyle;
use crate::core::{ImageHost, RuntimeError};

/// Cesium's default style: `IonWorldImageryStyle.AERIAL` (ion asset 2).
pub const DEFAULT_WORLD_IMAGERY_STYLE: IonWorldImageryStyle = IonWorldImageryStyle::Aerial;

#[derive(Default)]
pub struct WorldImageryOptions {
    pub style: Option<IonWorldImageryStyle>,
    pub ion: IonImageryProviderOptions,
}

/// The ion asset id for a supported world imagery style.
pub fn world_imagery_asset_id(style: IonWorldImageryStyle) -> u64 {
    style as u64
}

/// The style selected by options, or `AERIAL`.
pub fn world_imagery_style(options: &WorldImageryOptions) -> IonWorldImageryStyle {
    options.style.unwrap_or(DEFAULT_WORLD_IMAGERY_STYLE)
}

/// Creates the world imagery provider. Equivalent to
/// `IonImageryProvider.fromAssetId(style)`, as upstream.
pub async fn create_world_imagery_async<H: ImageHost + 'static>(
    loader: ImageryLoader<H>,
    options: WorldImageryOptions,
) -> Result<IonImageryProvider, RuntimeError> {
    let style = world_imagery_style(&options);
    create_async(loader, style, options.ion).await
}

/// The arguments `createWorldImageryAsync` would use, so a host can schedule
/// or inspect the request without starting it.
pub struct WorldImageryRequest {
    pub style: IonWorldImageryStyle,
    pub options: IonImageryProviderOptions,
}

impl WorldImageryRequest {
    pub fn new(options: WorldImageryOptions) -> Self {
        Self { style: world_imagery_style(&options), options: options.ion }
    }

    pub fn asset_id(&self) -> u64 {
        world_imagery_asset_id(self.style)
    }

    pub async fn create<H: ImageHost + 'static>(
        self,
        loader: ImageryLoader<H>,
    ) -> Result<IonImageryProvider, RuntimeError> {
        create_async(loader, self.style, self.options).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_styles_use_their_asset_ids() {
        assert_eq!(world_imagery_asset_id(IonWorldImageryStyle::Aerial), 2);
        assert_eq!(world_imagery_asset_id(IonWorldImageryStyle::AerialWithLabels), 3);
        assert_eq!(world_imagery_asset_id(IonWorldImageryStyle::Road), 4);
    }
}
