mod common;
use cesium_rs::core::resource::DerivedResourceOptions;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
fn complete<T>(f: impl Future<Output = T>) -> T {
    let mut f = pin!(f);
    for _ in 0..100 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            return v;
        }
    }
    panic!("fixture suspended")
}
#[derive(Default)]
struct Transport {
    requests: RefCell<Vec<HttpRequest>>,
    response: RefCell<Value>,
}
impl ResourceTransport for Transport {
    fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
        self.requests.borrow_mut().push(r);
        let body = self.response.borrow().to_string().into_bytes();
        Box::pin(async { Ok(HttpResponse { status: 200, body, headers: vec![] }) })
    }
}
fn map(headers: &[(String, String)]) -> Value {
    headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect()
}
#[test]
fn ion() {
    run(
        "core",
        "Ion",
        |_| EXACT,
        |_, _| {
            let ion = Ion::default();
            let c = Ion::get_default_token_credit(ion.default_access_token.as_deref())?;
            ok(json!([
                ion.default_server.url(),
                ion.default_access_token.is_some(),
                c.html(),
                c.show_on_screen,
                Ion::get_default_token_credit(Some("own-token")).map(|c| json!(c.html()))
            ]))
        },
    );
}
#[test]
fn google_maps() {
    run(
        "core",
        "GoogleMaps",
        |_| EXACT,
        |_, _| {
            let g = GoogleMaps::default();
            let c = GoogleMaps::get_default_credit();
            ok(json!([
                g.default_api_key,
                g.map_tiles_api_endpoint.url(),
                g.default_street_view_static_api_key,
                g.street_view_static_api_endpoint.url(),
                c.html(),
                c.show_on_screen
            ]))
        },
    );
}
#[test]
fn ion_geocoder() {
    run(
        "core",
        "IonGeocoderService",
        |_| EXACT,
        |_, a| {
            let p = IonGeocodeProviderType::from_str_value(s(&a[0]))?;
            let g = IonGeocoderService::new(
                &Ion::default(),
                IonGeocoderOptions {
                    access_token: Some("own token".into()),
                    server: Some(Resource::new("https://ion.test/api/")?),
                    provider: Some(p),
                },
            );
            let values: Vec<_> =
                [p, IonGeocodeProviderType::Google, IonGeocodeProviderType::Bing, IonGeocodeProviderType::Default]
                    .into_iter()
                    .map(|p| {
                        g.set_provider(p);
                        json!([g.provider().as_str(), g.resource().url()])
                    })
                    .collect();
            ok(json!([
                g.access_token,
                g.server.url(),
                g.static_credits.iter().map(|c| c.html()).collect::<Vec<_>>(),
                values
            ]))
        },
    );
}
#[test]
fn ion_resource() {
    run(
        "core",
        "IonResource",
        |_| EXACT,
        |_, a| {
            let t = Rc::new(Transport { response: RefCell::new(json!("ok")), ..Default::default() });
            let client = ResourceClient::new(t.clone());
            let ep = IonResource::create_endpoint_resource(
                42,
                &Ion::default(),
                &IonAssetOptions {
                    server: Some(Resource::new("https://ion.test/api/")?),
                    access_token: Some("own token".into()),
                    query_parameters: vec![(
                        "extra".into(),
                        Some(cesium_rs::core::query::QueryValue::Single("x".into())),
                    )],
                },
            );
            let r = match IonResource::new(a[0].clone(), ep.clone(), client.clone(), Ion::default().client_headers()) {
                Ok(r) => r,
                Err(e) => return Some(Out::Err(e.to_string())),
            };
            complete(r.fetch_text(&client)?).unwrap();
            let d =
                r.get_derived_resource(DerivedResourceOptions { url: Some("child/a".into()), ..Default::default() })?;
            complete(d.fetch_text(&client)?).unwrap();
            let foreign = r.get_derived_resource(DerivedResourceOptions {
                url: Some("https://other.test/a".into()),
                ..Default::default()
            })?;
            complete(foreign.fetch_text(&client)?).unwrap();
            let calls = t.requests.borrow().iter().map(|r| json!([r.url, map(&r.headers)])).collect::<Vec<_>>();
            let ep_headers = map(&ep.options().headers);
            ok(json!([
                ep.url(),
                ep_headers,
                r.url(),
                r.credits().iter().map(|c| json!([c.html(), c.show_on_screen])).collect::<Vec<_>>(),
                d.url(),
                calls
            ]))
        },
    );
}
#[test]
fn ion_derived_resource_keeps_caller_retry_options() {
    let t = Rc::new(Transport { response: RefCell::new(json!("ok")), ..Default::default() });
    let client = ResourceClient::new(t);
    let ep = IonResource::create_endpoint_resource(
        42,
        &Ion::default(),
        &IonAssetOptions { server: Some(Resource::new("https://ion.test/api/").unwrap()), ..Default::default() },
    );
    let endpoint = json!({"url": "https://assets.test/root/tileset.json", "accessToken": "t", "attributions": []});
    let r = IonResource::new(endpoint, ep, client, Ion::default().client_headers()).unwrap();
    let callback: cesium_rs::core::resource::RetryCallback = Rc::new(|_, _| Box::pin(async { true }));
    let d = r
        .get_derived_resource(DerivedResourceOptions {
            retry_attempts: Some(5),
            retry_callback: Some(callback.clone()),
            ..Default::default()
        })
        .unwrap();
    let o = d.as_resource().options();
    assert_eq!(o.retry_attempts, 5);
    assert!(o.fallible_retry_callback.is_none());
    assert!(Rc::ptr_eq(o.retry_callback.as_ref().unwrap(), &callback));
    drop(o);
    // Without overrides, the derived resource keeps the ion token refresh.
    let plain = r.get_derived_resource(DerivedResourceOptions::default()).unwrap();
    let o = plain.as_resource().options();
    assert_eq!(o.retry_attempts, 1);
    assert!(o.fallible_retry_callback.is_some());
}
#[test]
fn itwin() {
    run(
        "core",
        "ITwinPlatform",
        |_| EXACT,
        |f, a| {
            let p = ITwinPlatform {
                default_access_token: Some("access-token".into()),
                default_share_key: a[0].as_str().map(str::to_owned),
                api_endpoint: Resource::new("https://itwin.test/api/")?,
                client_version: Some("1.139.0".into()),
            };
            if f == "error" {
                let e = RequestErrorEvent::new(
                    Some(u(&a[0]) as u16),
                    Some(
                        serde_json::to_vec(&json!({"error":{"code":"C","message":"M","details":[{"code":"D"}]}}))
                            .unwrap(),
                    ),
                    None,
                );
                return Some(Out::Err(
                    ITwinPlatform::map_error(&e, a[1].as_bool().unwrap().then_some(("twin", "data"))).to_string(),
                ));
            }
            let kind = s(&a[1]);
            let r = match kind {
                "exports" => p.exports_resource("model", Some("change")),
                "metadata" => p.metadata_resource("twin", "data"),
                _ => p.read_access_resource("twin", "data"),
            }
            .unwrap();
            let result = match kind {
                "exports" => json!({"exports":[]}),
                "metadata" => json!({"id":"data-id"}),
                _ => json!(ITwinPlatform::reality_data_url(
                    &json!({"_links":{"containerUrl":{"href":"https://blob.test/container?sig=a"}}}),
                    "folder/a b.json"
                )
                .unwrap()),
            };
            let headers = map(&r.options().headers);
            ok(json!([[r.url(), headers], result]))
        },
    );
}
#[test]
fn ion_refresh_propagates_errors_and_never_leaks_credentials() {
    struct Auth {
        calls: RefCell<Vec<HttpRequest>>,
        fail_refresh: bool,
    }
    impl ResourceTransport for Auth {
        fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
            let endpoint = r.url.contains("/endpoint");
            self.calls.borrow_mut().push(r.clone());
            let result = if endpoint {
                if self.fail_refresh {
                    Err(RequestErrorEvent::new(Some(503), Some(b"refresh failed".to_vec()), None))
                } else {
                    Ok(HttpResponse {
                        status: 200,
                        body: serde_json::to_vec(
                            &json!({"url":"https://assets.test/a/","accessToken":"new","attributions":[]}),
                        )
                        .unwrap(),
                        headers: vec![],
                    })
                }
            } else if r.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer old") {
                Err(RequestErrorEvent::new(Some(401), None, None))
            } else {
                Ok(HttpResponse { status: 200, body: b"ok".to_vec(), headers: vec![] })
            };
            Box::pin(async { result })
        }
    }
    for fail_refresh in [false, true] {
        let transport = Rc::new(Auth { calls: Default::default(), fail_refresh });
        let client = ResourceClient::new(transport.clone());
        let root = IonResource::new(
            json!({"url":"https://assets.test/a/","accessToken":"old","attributions":[]}),
            Resource::new("https://ion.test/endpoint").unwrap(),
            client.clone(),
            Ion::default().client_headers(),
        )
        .unwrap();
        let result = complete(root.fetch_text(&client).unwrap());
        if fail_refresh {
            assert_eq!(result.unwrap_err().status_code, Some(503));
        } else {
            assert_eq!(result.unwrap(), Some("ok".into()));
            assert_eq!(root.endpoint()["accessToken"], "new");
        }
        let foreign = root
            .get_derived_resource(DerivedResourceOptions {
                url: Some("https://other.test/child".into()),
                ..Default::default()
            })
            .unwrap();
        complete(foreign.fetch_text(&client).unwrap()).unwrap();
        assert!(!transport.calls.borrow().last().unwrap().headers.iter().any(|(k, _)| k == "Authorization"));
    }
}
