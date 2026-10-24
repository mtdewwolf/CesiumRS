# DataSources

The first **7 of 108 DataSources modules** are ported against `@cesium/engine`
23.0.0. They provide the property layer needed by Entity, graphics descriptions
and the document loaders. Entity, visualizers, CZML, KML, GeoJSON and GPX remain
unported. Scene status is tracked separately in [scene.md](scene.md).

| Cesium module | Rust API | Behavior |
| --- | --- | --- |
| `Property` | `data_sources::Property<T>` and `property` helpers | Optional values, equality, array equality, constant checks, defaults and cloned defaults |
| `ConstantProperty` | `ConstantProperty<T>` | Stored values, change suppression, equality, value extraction and string conversion |
| `CallbackProperty` | `CallbackProperty<T>` | Lazy callbacks, supplied time/result, callback identity and definition changes |
| `PropertyArray` | `PropertyArray<T>` | Child evaluation, undefined/null filtering, compacted results and forwarded changes |
| `SampledProperty` | `SampledProperty<T: SampledValue>` | Sorted/replaced samples, packed inputs, interpolation, derivatives, extrapolation, sample access and removal |
| `TimeIntervalCollectionProperty` | `TimeIntervalCollectionProperty<T>` | Raw interval values, endpoint inclusion, overlap/removal and collection changes |
| `CompositeProperty` | `CompositeProperty<T>` | Interval-selected properties and subscription rewiring when intervals change |

## Values, time and ownership

`PropertyRef<T>` is `Rc<dyn Property<T>>`. Properties and their change listeners
are local to one thread. `get_value` returns `Result<Option<T>, RuntimeError>`;
`None` represents JavaScript undefined. Choose `T = Option<U>` when explicit
null must also be represented. PropertyArray filters both missing and null values.

Core math values use owned clones. Passing a mutable result slot updates that
slot when a value exists. Extracted values, including ConstantProperty's
`value_of()`, are owned snapshots. For shared non-cloneable JavaScript object or
array behavior, use `Rc<T>` or `JsonReference`; those compare by reference
identity. An owned Rust `Vec<T>` uses structural equality. `PropertyValue`
allows application types to provide their own equality and string conversion.

Omitted times use the system clock on native targets and JavaScript `Date.now()`
with the WASM `browser` feature. Bare WASM hosts must pass a time or inject a
`PropertyClock` through `with_clock`. Arrays and composites resolve the time once
and pass that same date to the selected children. ConstantProperty ignores time.

```rust
use cesium_rs::{core::JulianDate, data_sources::{Property, SampledProperty}};

let epoch = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
let altitude = SampledProperty::<f64>::new(None);
altitude.add_samples(&[epoch, epoch.add_seconds(10.)], &[100., 200.], None).unwrap();
assert_eq!(altitude.get_value(Some(&epoch.add_seconds(5.)), None).unwrap(), Some(150.));
```

## Events and intervals

Keep the `Subscription` returned by `definition_changed().add_listener(...)`
alive for as long as the listener is needed. Removing or dropping it unregisters
the callback. Subscribing the same `Rc` callback twice addresses one registration;
dropping either handle removes it. Closures capture their required property or
application state; the callback argument is the emitting `DefinitionChanged`
event. Weak forwarding prevents child/parent ownership cycles, and dropping an
array or composite removes its child subscriptions.

Listener additions and removals during dispatch settle after that dispatch.
A listener removed by an earlier listener still runs during the current dispatch.
Renewing a removed subscription creates a new registration, so deferred removal
and stale handles cannot erase its replacement. Duplicate subscriptions share
one registration and listener count, including during dispatch. This local
adapter supports `Rc` callbacks; Core's existing `Send` event API is unchanged.
Mutation completes before property notifications, allowing observers to query or
change the new definition.

PropertyArray evaluates a snapshot of its children. A child callback can replace,
shorten or clear the array: the current evaluation finishes with the original
children, and the next evaluation uses the replacement. These two reentrancy
contracts deliberately differ from pinned CesiumJS, which can lose a renewed
subscription and can mix array definitions or throw during evaluation. Native
regression tests cover renewal/cancellation, stale handles, repeated forwarded
changes and all replacement lengths.

`PropertyIntervals` delegates interval algorithms to Core and emits local events
after releasing the collection's mutable borrow. Its `read()` guard exposes Core
queries; drop the guard before calling `add_interval`, `remove_interval` or
`remove_all`. Raw interval data is `Rc<T>`, preserving reference identity when
merging adjacent object values. Composite interval data is `PropertyRef<T>`.
Equality compares values through Cesium's property equality helper. Its raw-data
quirk is preserved: comparing different primitive interval values can return an
error corresponding to JavaScript's `left.equals is not a function`.

## Sampling

`SampledValue` reuses Core packing for scalar numbers, Cartesian vectors, colors,
matrices, quaternions and the other supported Core packable values. Application
codecs supply packing and optional interpolation transforms. Quaternion samples
use rotation-vector conversion, then reconstruct the interpolated quaternion.

`Interpolation` selects Linear, Lagrange, Hermite or a custom Core
`InterpolationAlgorithm`. `SampleType::of::<T>()` describes derivative types;
derivative values are packed numeric rows in the order declared. Hermite consumes
those derivatives; algorithms without higher-order interpolation use order zero.
`PackedSampleValue` supports epoch-relative numbers, Julian dates and ISO 8601
timestamps. Numeric timestamps require an epoch.

Duplicate sample times overwrite previous values. Bulk operations emit one change
notification. Removing a missing individual sample emits none; interval removal
emits even when it removes no samples. Extrapolation defaults to NONE; HOLD and
EXTRAPOLATE honor durations, with zero meaning unlimited. Equality intentionally
ignores extrapolation settings, matching Cesium. The pinned forward-HOLD packed
offset with derivative samples is also preserved and reference-tested.

## Validation and remaining work

`tests/data_sources_properties.rs` replays **206 pinned CesiumJS scenarios** across
all seven modules. Sampling covers out-of-order/duplicate times, three algorithms,
four degrees, scalar/vector derivatives, quaternion conversion, all extrapolation
modes, durations, numeric/date/ISO packed inputs and removals. Interval fixtures
cover inclusions, overlap/splitting, merging, equality and subscription changes.
Additional fixtures exercise listener edits and nested dispatch.

`tests/data_sources_lifecycle.rs` adds **14 native integration tests** for result
ownership, callbacks replacing themselves, observers reading/mutating committed
state, custom interpolation caching, coherent clocks, listener cleanup and weak
ownership and reentrant array/subscription changes. The Chromium Core harness
executes all five time-dependent property classes with omitted times through the
browser WASM adapter.

Next, implement position/velocity/orientation properties, the property bag and
entity collections. Then port graphics definitions, data-source collections and
the loaders in dependency order. The largest document modules are CzmlDataSource
(5,142 upstream lines), KmlDataSource (4,249), GeoJsonDataSource (1,064) and
GpxDataSource (1,018). Their property/entity dependencies must exist before their
loader behavior can be completed and validated.
