//! Ion geocoding delegates result parsing to Pelias. Static credits are
//! returned to the host instead of reaching into a Scene credit display.
use super::geocoder_service::{GeocoderFuture, GeocoderHost, GeocoderResult, PeliasGeocoderService};
use super::query::QueryValue;
use super::resource::DerivedResourceOptions;
use super::{Credit, GeocodeType, GeocoderService, Ion, IonGeocodeProviderType, Resource};
#[derive(Default)]
pub struct IonGeocoderOptions {
    pub access_token: Option<String>,
    pub server: Option<Resource>,
    pub provider: Option<IonGeocodeProviderType>,
}
pub struct IonGeocoderService {
    pub access_token: Option<String>,
    pub server: Resource,
    pub static_credits: Vec<Credit>,
    pelias: PeliasGeocoderService,
}
impl IonGeocoderService {
    pub fn new(ion: &Ion, o: IonGeocoderOptions) -> Self {
        let token = o.access_token.or_else(|| ion.default_access_token.clone());
        let server = o.server.unwrap_or_else(|| ion.default_server.clone());
        server.append_forward_slash();
        let endpoint = server
            .get_derived_resource(DerivedResourceOptions { url: Some("v1/geocode".into()), ..Default::default() })
            .unwrap();
        if let Some(token) = &token {
            endpoint.append_query_parameters(&vec![("access_token".into(), Some(QueryValue::Single(token.clone())))]);
        }
        let static_credits = Ion::get_default_token_credit(token.as_deref()).into_iter().collect();
        let result = Self { access_token: token, server, static_credits, pelias: PeliasGeocoderService::new(endpoint) };
        result.set_provider(o.provider.unwrap_or(IonGeocodeProviderType::Default));
        result
    }
    pub fn resource(&self) -> &Resource {
        &self.pelias.resource
    }
    pub fn provider(&self) -> IonGeocodeProviderType {
        let o = self.pelias.resource.options();
        match o.query_parameters.iter().find(|(k, _)| k == "geocoder").and_then(|(_, v)| v.as_ref()) {
            Some(QueryValue::Single(s)) if s == "google" => IonGeocodeProviderType::Google,
            Some(QueryValue::Single(s)) if s == "bing" => IonGeocodeProviderType::Bing,
            _ => IonGeocodeProviderType::Default,
        }
    }
    pub fn set_provider(&self, p: IonGeocodeProviderType) {
        let mut q = self.pelias.resource.options().query_parameters.clone();
        q.retain(|(k, _)| k != "geocoder");
        if p != IonGeocodeProviderType::Default {
            q.push((
                "geocoder".into(),
                Some(QueryValue::Single(if p == IonGeocodeProviderType::Google { "google" } else { "bing" }.into())),
            ));
        }
        self.pelias.resource.set_query_parameters(&q, false);
    }
    pub fn request(&self, q: &str, t: GeocodeType) -> Result<Resource, super::RuntimeError> {
        self.pelias.request(q, t)
    }
}
impl GeocoderService for IonGeocoderService {
    fn credit(&self) -> Option<&Credit> {
        None
    }
    fn geocode<'a>(
        &'a self,
        q: &'a str,
        t: GeocodeType,
        h: &'a dyn GeocoderHost,
    ) -> GeocoderFuture<'a, Vec<GeocoderResult>> {
        self.pelias.geocode(q, t, h)
    }
}
