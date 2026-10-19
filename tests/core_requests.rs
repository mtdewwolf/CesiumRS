mod common;
use cesium_rs::core::resource::*;
use cesium_rs::core::resource_client::*;
use cesium_rs::core::*;
use common::*;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    pin::{pin, Pin},
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
fn poll<F: Future + Unpin>(f: &mut F) -> Poll<F::Output> {
    Pin::new(f).poll(&mut Context::from_waker(Waker::noop()))
}
fn complete<T>(f: impl Future<Output = T>) -> T {
    let mut f = pin!(f);
    for _ in 0..100 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            return v;
        }
    }
    panic!("ready fixture stalled")
}
#[test]
fn scheduler() {
    run(
        "core",
        "RequestScheduler",
        |_| EXACT,
        |_, a| {
            let mut scheduler = RequestScheduler::new();
            scheduler.maximum_requests = 2;
            scheduler.maximum_requests_per_server = 1;
            scheduler.set_priority_heap_length(3);
            let started = Rc::new(RefCell::new(vec![]));
            let cancelled = Rc::new(RefCell::new(vec![]));
            let completed = Arc::new(Mutex::new(vec![]));
            let completed_event = completed.clone();
            scheduler.request_completed_event.add_event_listener(move |e| {
                completed_event.lock().unwrap().push(if e.is_some() { "error" } else { "ok" })
            });
            let mut requests = vec![];
            let mut responses = vec![];
            let mut sends = vec![];
            for (i, priority) in nums(&a[1]).into_iter().enumerate() {
                let (tx, rx) = futures_channel::oneshot::channel();
                sends.push(Some(tx));
                let rx = RefCell::new(Some(rx));
                let start = started.clone();
                let cancel = cancelled.clone();
                let r = Request::new(RequestOptions {
                    url: format!("https://request.test/{i}"),
                    priority,
                    throttle: true,
                    request_function: Some(Rc::new(move || {
                        start.borrow_mut().push(i);
                        let rx = rx.borrow_mut().take().unwrap();
                        Box::pin(async move { rx.await.unwrap() })
                    })),
                    cancel_function: Some(Rc::new(move || cancel.borrow_mut().push(i))),
                    ..Default::default()
                })
                .into_handle();
                responses.push(scheduler.request(r.clone()));
                requests.push(r);
            }
            let snapshot = |s: &RequestScheduler<usize>| {
                let stats = &s.statistics;
                json!([
                    requests.iter().map(|r| r.borrow().state as u8).collect::<Vec<_>>(),
                    *started.borrow(),
                    *cancelled.borrow(),
                    *completed.lock().unwrap(),
                    [
                        stats.number_of_attempted_requests,
                        stats.number_of_active_requests,
                        stats.number_of_cancelled_requests,
                        stats.number_of_cancelled_active_requests,
                        stats.number_of_failed_requests,
                        stats.number_of_active_requests_ever
                    ]
                ])
            };
            let mut steps = vec![snapshot(&scheduler)];
            let mut cx = Context::from_waker(Waker::noop());
            scheduler.poll_update(&mut cx);
            steps.push(snapshot(&scheduler));
            for i in started.borrow().iter().copied() {
                match s(&a[0]) {
                    "cancel" => requests[i].borrow_mut().cancel(),
                    "failure" => {
                        sends[i].take().unwrap().send(Err(RequestErrorEvent::new(None, None, None))).ok();
                    }
                    _ => {
                        sends[i].take().unwrap().send(Ok(i)).ok();
                    }
                }
            }
            scheduler.poll_update(&mut cx);
            steps.push(snapshot(&scheduler));
            for i in started.borrow().iter().copied() {
                if let Some(tx) = sends[i].take() {
                    tx.send(Ok(i)).ok();
                }
            }
            scheduler.poll_update(&mut cx);
            steps.push(snapshot(&scheduler));
            ok(json!(steps))
        },
    );
}
struct Transport {
    responses: RefCell<VecDeque<Result<HttpResponse, RequestErrorEvent>>>,
    requests: RefCell<Vec<HttpRequest>>,
    cancelled: RefCell<Vec<u64>>,
}
impl Transport {
    fn new(r: Vec<Result<HttpResponse, RequestErrorEvent>>) -> Rc<Self> {
        Rc::new(Self {
            responses: RefCell::new(r.into()),
            requests: RefCell::new(vec![]),
            cancelled: RefCell::new(vec![]),
        })
    }
}
impl ResourceTransport for Transport {
    fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
        self.requests.borrow_mut().push(r);
        let result = self.responses.borrow_mut().pop_front();
        match result {
            Some(r) => Box::pin(async { r }),
            None => Box::pin(std::future::pending()),
        }
    }
    fn cancel(&self, r: &HttpRequest) {
        self.cancelled.borrow_mut().push(r.id);
    }
}
fn response(status: u16, body: &[u8]) -> HttpResponse {
    HttpResponse { status, body: body.to_vec(), headers: vec![("Content-Type".into(), "application/json".into())] }
}
#[test]
fn resource_retries_release_slots_and_reapply_credentials() {
    let transport = Transport::new(vec![
        Err(RequestErrorEvent::new(Some(503), Some(b"busy".to_vec()), None)),
        Ok(response(200, b"{\"ok\":true}")),
    ]);
    let client = ResourceClient::new(transport.clone());
    client.trusted_servers.borrow_mut().add("request.test", 443);
    let attempts = Rc::new(RefCell::new(vec![]));
    let seen = attempts.clone();
    let scheduler = client.scheduler.clone();
    let r = Resource::from_options(ResourceOptions {
        url: "https://request.test/a".into(),
        headers: vec![("X-Test".into(), "a".into())],
        retry_attempts: 1,
        retry_callback: Some(Rc::new(move |r, e| {
            assert_eq!(scheduler.borrow().statistics.number_of_active_requests, 0);
            assert_eq!(e.status_code, Some(503));
            seen.borrow_mut().push(r.url());
            r.set_query_parameters(
                &vec![("retry".into(), Some(cesium_rs::core::query::QueryValue::Single("yes".into())))],
                false,
            );
            Box::pin(async { true })
        })),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(complete(r.fetch_json(&client).unwrap()).unwrap(), Some(json!({"ok":true})));
    assert_eq!(attempts.borrow().len(), 1);
    let req = transport.requests.borrow();
    assert_eq!(req.len(), 2);
    assert_ne!(req[0].id, req[1].id);
    assert!(req.iter().all(|r| r.with_credentials));
    assert!(req[1].url.ends_with("?retry=yes"));
    assert!(req[1].headers.contains(&("Accept".into(), "application/json,*/*;q=0.01".into())));
    assert_eq!(r.request.borrow().state, RequestState::Received);
}
#[test]
fn resource_cancellation_does_not_retry() {
    let t = Transport::new(vec![]);
    let client = ResourceClient::new(t.clone());
    let r = Resource::from_options(ResourceOptions {
        url: "https://request.test/a".into(),
        retry_attempts: 1,
        retry_callback: Some(Rc::new(|_, _| panic!("cancelled request retried"))),
        ..Default::default()
    })
    .unwrap();
    let mut f = r.fetch_text(&client).unwrap();
    assert!(poll(&mut f).is_pending());
    r.request.borrow_mut().cancel();
    assert!(complete(f).is_err());
    assert_eq!(t.cancelled.borrow().len(), 1);
    assert_eq!(client.scheduler.borrow().statistics.number_of_active_requests, 0);
}
#[test]
fn typed_resource_decoders_and_no_content() {
    let t = Transport::new(vec![
        Ok(HttpResponse {
            status: 200,
            body: vec![0xe9],
            headers: vec![("Content-Type".into(), "text/plain; charset=windows-1252".into())],
        }),
        Ok(response(204, b"")),
        Ok(response(200, b"<root><v>ok</v></root>")),
        Ok(response(200, b"not xml")),
        Ok(response(204, b"")),
    ]);
    let client = ResourceClient::new(t);
    let r = Resource::new("https://request.test/a").unwrap();
    assert_eq!(complete(r.fetch_text(&client).unwrap()).unwrap(), Some("é".into()));
    assert_eq!(complete(r.fetch_json(&client).unwrap()).unwrap(), None);
    let xml = complete(r.fetch_xml(&client).unwrap()).unwrap().unwrap();
    assert_eq!(xml.document().root_element().tag_name().name(), "root");
    assert!(complete(r.fetch_xml(&client).unwrap()).is_err());
    assert_eq!(complete(r.head(&client).unwrap()).unwrap(), vec![("Content-Type".into(), "application/json".into())]);
}
#[test]
fn data_uris_and_jsonp_parser() {
    for (uri, text) in
        [("data:text/plain,a%20b", "a b"), ("data:text/plain;base64,6Q==", "é"), ("data:text/plain,%F0%9F%8C%8D", "🌍")]
    {
        assert_eq!(decode_data_uri_text(uri).unwrap(), text);
    }
    assert!(data_uri_response("data:text/plain,%ZZ").is_err());
    assert_eq!(parse_jsonp(" /* comment */ cb({\"ok\":true}); ", "cb").unwrap(), json!({"ok":true}));
    for js in ["evil({});", "cb({});alert(1)", "cb(undefined)"] {
        assert!(parse_jsonp(js, "cb").is_err());
    }
}
#[test]
fn request_clone_resets_execution() {
    let mut r = Request::<()>::new(RequestOptions { priority: 5., throttle: true, ..Default::default() });
    r.cancel();
    r.state = RequestState::Failed;
    let copy = r.clone();
    assert!(!copy.cancelled);
    assert_eq!(copy.state, RequestState::Unissued);
    assert_eq!(copy.options.priority, 5.);
    assert!(copy.options.throttle);
}
#[test]
fn per_server_limits_are_independent() {
    let mut s = RequestScheduler::<()>::new();
    s.maximum_requests_per_server = 1;
    let make = |url: &str| {
        Request::new(RequestOptions {
            url: url.into(),
            throttle_by_server: true,
            request_function: Some(Rc::new(|| Box::pin(std::future::pending()))),
            ..Default::default()
        })
        .into_handle()
    };
    assert!(s.request(make("https://a.test/one")).is_some());
    assert!(s.request(make("https://a.test/two")).is_none());
    assert!(s.request(make("https://b.test/one")).is_some());
    assert_eq!(s.statistics.number_of_active_requests, 2);
    s.clear();
    assert_eq!(s.statistics.number_of_active_requests, 0);
}
