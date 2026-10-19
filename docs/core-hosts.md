# Core host adapters

The portable Core algorithms use explicit host capabilities. `basisu` supplies native Rust ETC1S/UASTC transcoding (including Zstd levels), so KTX2 decoding does not require an external JavaScript codec. Browser operations use a small optional DOM/worker bridge; they do not bind to CesiumJS algorithms.

These implementations have passed compile/build checks only. No parity tests or fixture regeneration were run for this batch, as requested. The tracker calls them **implemented, unchecked** and keeps validated coverage separate.

## Native I/O and tasks

Enable `native-host` and run returned futures using your application's executor:

```rust,ignore
use cesium_rs::core::{Resource, ResourceClient};
use cesium_rs::core::native_host::NativeTransport;
use std::rc::Rc;

let client = ResourceClient::new(Rc::new(NativeTransport::default()));
let resource = Resource::new("https://example.com/data.json").unwrap();
let response = resource.fetch_json(&client).unwrap().await?;
// response is Option<Value>: None represents an absent response.
```

`Resource` construction/derivation copies URL/query/template/header/credit data. `create_if_needed` preserves the caller's request handle. Ordinary cloning creates an unissued request. Fetch methods return `None` immediately when scheduling defers a request; the future itself can resolve to `None` for a 204 response or a deferred retry. `ResourceClient` also offers strict conveniences which reject an absent response when the caller requires data.

`ResourceClient` polls its shared scheduler while waiting for responses. Retries release a failed request's slot and pass through the scheduler again. Cancellation rejects the pending response and bypasses retry policy. Direct `RequestScheduler` users call `poll_update` with the application's task context.

`NativeTransport` runs blocking HTTP on background threads. It retains response bodies and headers for HTTP errors. Dropping/cancelling a native request stops consuming its response on the next read; an already-blocking system/network read cannot be forcefully interrupted by safe Rust. Browser XHR/fetch cancellation aborts the actual request.

`NativeTimer` supplies asynchronous sampling backoff without blocking the executor. `NativeTaskWorker` invokes application-supplied Rust task functions on background threads; those callbacks receive a cooperative cancellation token. Its initialization callback configures the application's native/module runtime. `BrowserTaskWorker` instead loads module workers and WebAssembly binaries from explicit paths.

Schedulers, resource clients and response handles use `Rc`/`RefCell` and belong to one event-loop thread. Native worker/HTTP jobs transfer owned data through channels. This is distinct from shared terrain error events, which retain the crate's `Arc<Mutex<Event<_>>>` contract. Tile retry-policy events use `MutableErrorEvent` so listeners can change `retry`; `TileErrorReporter` dispatches them or logs the provider error.

## Browser package

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
scripts/build-browser-core.sh
```

Serve `target/browser-core` as static files. It contains the WebAssembly, generated bindings, host module, DOMPurify module and license notices:

```javascript
import init, { BrowserCore } from './cesium_rs.js';
await init();
const core = new BrowserCore();
const data = await core.fetchJson('/data.json');
const image = await core.fetchImage('/image.png', false, false);
const credit = core.creditElement('<a href="https://example.com">Data</a>', false);
```

Rust consumers can use `BrowserTransport`, `BrowserImageHost`, `BrowserCreditRenderer`, `BrowserTimer`, `BrowserTaskWorker` and `BrowserVideoBinding` directly. Worker paths are absolute URLs or module IDs resolved under `Workers/` relative to the supplied base URL. The task protocol matches Cesium worker messages; application task parameters/results are typed JSON. Native Rust ownership replaces JS transferable-object lists.

Credits are sanitized by the pinned DOMPurify distribution and rendered as inline credit wrappers with blank-target links. Each Credit instance caches its own element; cloning keeps the HTML identity but creates a separate DOM-element identity. Cache entries are released after their owning credits are dropped.

`BrowserImageHost` uses canvas copy/readback and browser resampling. Typed-array image decoding probes bitmap/color-option support, supplies unpremultiplied output, and falls back to an HTML image when necessary. Resource image fetching separately preserves `prefer_blob`/`prefer_image_bitmap`, direct image loads, blob attachment, flip/color flags and object-URL cleanup. The typed-array view overload intentionally uses its entire backing buffer, matching Cesium's offset quirk.

`BrowserVideoBinding` owns clock and media subscriptions. Changing its element or clock removes the prior listener; dropping/destroying it releases both. Full clock snapshots avoid borrowing a clock while it dispatches an event. The original date-only clock event remains available. Video synchronization preserves Cesium's negative-time loop arithmetic.

## Terrain sampling and undefined values

Sampling groups positions by tile, starts requests concurrently, backs off asynchronously on throttling, and creates a non-exaggerated mesh when raw interpolation is unavailable. Heights assigned before a mesh/request failure are retained when rejecting; the default non-rejecting policy marks failed tile heights as NaN. Rust's numeric Cartographic height uses NaN where JavaScript sets undefined. Positions outside the tiling scheme remain unchanged.

TileAvailability permits range insertion through shared references using an interior tree lock. Provider futures can therefore discover availability while sampling is in progress. Most-detailed sampling snapshots levels, awaits availability loads, samples level groups concurrently and revisits points whose maximum available level changed.
