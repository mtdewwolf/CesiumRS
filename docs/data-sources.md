# DataSources

The first **23 of 108 DataSources modules** are ported against `@cesium/engine`
23.0.0. They provide values, positions, motion, property bags, entities, collections
and references. Concrete graphics classes, composite collections, visualizers,
CZML, KML, GeoJSON and GPX remain unported. Scene status is tracked separately in [scene.md](scene.md).

| Cesium module | Rust API | Behavior |
| --- | --- | --- |
| `Property` | `data_sources::Property<T>` and `property` helpers | Optional values, equality, array equality, constant checks, defaults and cloned defaults |
| `ConstantProperty` | `ConstantProperty<T>` | Stored values, change suppression, equality, value extraction and string conversion |
| `CallbackProperty` | `CallbackProperty<T>` | Lazy callbacks, supplied time/result, callback identity and definition changes |
| `PropertyArray` | `PropertyArray<T>` | Child evaluation, undefined/null filtering, compacted results and forwarded changes |
| `SampledProperty` | `SampledProperty<T: SampledValue>` | Sorted/replaced samples, packed inputs, interpolation, derivatives, extrapolation, sample access and removal |
| `TimeIntervalCollectionProperty` | `TimeIntervalCollectionProperty<T>` | Raw interval values, endpoint inclusion, overlap/removal and collection changes |
| `CompositeProperty` | `CompositeProperty<T>` | Interval-selected properties and subscription rewiring when intervals change |
| `PositionProperty` | `PositionProperty` and `PositionContext` | Fixed/inertial positions, shared clocks, Earth data and custom central-body transforms |
| `ConstantPositionProperty` | `ConstantPositionProperty` | Stored position/frame, conversion, equality and atomic changes |
| `CallbackPositionProperty` | `CallbackPositionProperty` | Lazy positions in their declared frame and callback identity |
| `SampledPositionProperty` | `SampledPositionProperty` | Cartesian samples/derivatives, interpolation/extrapolation and frame conversion |
| `TimeIntervalCollectionPositionProperty` | `TimeIntervalCollectionPositionProperty` | Raw Cartesian intervals evaluated in a requested frame |
| `CompositePositionProperty` | `CompositePositionProperty` | Interval-selected position properties and child notifications |
| `PositionPropertyArray` | `PositionPropertyArray` | Mixed-frame children, missing-value compaction and shared evaluation time |
| `ScaledPositionProperty` | `ScaledPositionProperty` | Ellipsoid geodetic-surface projection of a position |
| `VelocityVectorProperty` | `VelocityVectorProperty` | Normalized direction or raw velocity with endpoint fallback |
| `VelocityOrientationProperty` | `VelocityOrientationProperty` | Motion-derived quaternion orientation and ellipsoid settings |
| `createPropertyDescriptor` | `create_property_descriptor` | Owned slots, value factories, identity suppression and child change forwarding |
| `createRawPropertyDescriptor` | `create_raw_property_descriptor` | Raw slots without automatic ConstantProperty wrapping |
| `PropertyBag` | `PropertyBag` | Ordered heterogeneous fields, result reuse, equality, clone/merge contracts and child events |
| `Entity` | `Entity` and `EntityTypeRegistry` | Component slots, hierarchy/show, availability, merge and model/height transforms |
| `EntityCollection` | `EntityCollection` | ID lookup, membership, batched/reentrant notifications, show and availability bounds |
| `ReferenceProperty` | `ReferenceProperty` | Lazy nested/escaped references, type/frame forwarding, equality and target changes |

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

## Positions and motion

`PositionProperty` extends `Property<Cartesian3>` with `reference_frame()` and
`get_value_in_reference_frame(time, frame, result)`. Its normal `get_value` always
returns fixed-frame coordinates. `PositionPropertyRef` is a shared trait object;
position references can also be used wherever `PropertyRef<Cartesian3>` is needed.

Constructors accept explicit frames and optional values; `Default` supplies
Cesium's defaults where construction needs no callback. Each `with_context`
constructor accepts a cloneable `PositionContext`. The context supplies an omitted-
time clock, a fallible ICRF-to-body-fixed transform and the default ellipsoid.
Same-frame and missing-value conversions skip the transform. `FrameConversion`
names the input/output frames for the static conversion helper.

The default context uses WGS84 and Core's TEME-to-pseudo-fixed fallback, matching
Cesium before IAU data is available. `PositionContext::with_earth_data` takes
`Rc<EarthOrientationParameters>` and `Rc<RefCell<Iau2006XysData>>`; it uses ICRF
when data is loaded and falls back while chunks are missing. Hosts fetch/preload
Core data or use `Iau2006XysData::bundled()` with `bundled-xys`. Properties see
subsequent chunks through the shared context. Custom `frame_transform` and
`ellipsoid` providers support another body, including Core's Moon transform.
These providers replace Cesium's mutable global transform/default-ellipsoid hooks.
Equality follows Cesium's stored-definition comparisons; host contexts are not
compared. Share a context among properties using the same body and time source.

```rust
use cesium_rs::{core::*, data_sources::*};

let start = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
let track = SampledPositionProperty::default();
track.add_samples(
    &[start, start.add_seconds(10.)],
    &[Cartesian3::new(6378137., 0., 0.), Cartesian3::new(6378137., 100., 0.)],
    None,
).unwrap();
let inertial = track.get_value_in_reference_frame(
    &start.add_seconds(5.), ReferenceFrame::Inertial, None,
).unwrap();
assert!(inertial.is_some());
```

Sampled positions delegate sample/settings operations to `SampledProperty`
through `Deref`. Their `add_sample` and `add_samples` accept Cartesian derivative
vectors; packed arrays use the same flat numeric layout as Cesium. Position
arrays snapshot their children and resolve omitted time once. Composites pass
the requested frame directly to the selected child. Their frame metadata setter
emits no event, matching Cesium. Scaling projects onto the context's ellipsoid
after the child has converted to the requested frame.

Velocity uses positions at the evaluation time and 1/60 second later. If the
later position is missing, it tries the previous 1/60 second. Normalized constant
or stationary positions return undefined; raw velocity returns zero. Velocity
equality intentionally ignores the normalization flag, matching Cesium.
Orientation uses Core's position/velocity rotation and quaternion conversion;
it also preserves the earlier position used by a backward difference. Each
evaluation owns its scratch values, allowing nested motion evaluations.

## Entities and components

`Entity::new(EntityOptions)` returns `Rc<Entity>`. It supports every built-in
slot name, named/custom properties, tracking frames, parents, show propagation,
availability, merging, model matrices and height-reference transforms. Matrix
methods take an explicit time and optional reusable result; terrain capability
queries take the renderer's depth-texture support. Availability reuses observable
`PropertyIntervals<()>`.

`PropertyInput` holds a raw `DynamicValue` or `Rc<dyn EntityComponent>`.
`DynamicValue` supports null, booleans, numbers, strings, Cartesian vectors,
quaternions, colors, matrices, dates, identity-bearing JSON and evaluated records.
`None` represents undefined. `PropertyComponent::wrap` adapts supported typed
properties; `PositionComponent::wrap` preserves position/frame behavior. Adapters
preserve the underlying property's identity across repeated wrapping.

`EntityComponent` supplies evaluation, definition events, equality and optional
clone/merge, nested-property, material-type and position contracts. Graphics slots
accept these typed components. **Concrete Graphics classes and automatic raw
JavaScript graphics-object conversion are not ported by this batch.** Position,
properties and ordinary property slots have their native conversions. Custom
`EntityTypeRegistry` factories are scoped and shared explicitly, replacing
JavaScript's mutable Entity prototype; registration affects subsequently created
entities. Raw custom fields use the raw descriptor contract.

`PropertyBag` preserves field insertion order and JavaScript integer-key ordering
for object inputs/results. Evaluation reads one clock value and snapshots fields;
removed fields no longer notify the bag. A supplied result retains unrelated keys.
Merge invokes a component's merge method when available, cloning an absent source
component only when both clone and merge are supported; otherwise it shares the
component. PropertyBag itself has no clone contract, matching Cesium.

`ChangeEvent<T>` adds named payloads with the same local `Subscription` semantics
as definition events. Entity changes expose weak entity pointers and old/new
inputs; collection changes contain owned added/removed/changed snapshots. Parent
edges are strong, child lists and collection back-pointers are weak, and parent
cycles are rejected. Collections own their entities and detach all membership
subscriptions on removal, including entities added and removed while suspended.
Callbacks run after mutable state borrows have been released.

`ReferenceProperty::from_string(collection, "vehicle#properties.speed")` resolves
lazily; backslashes escape dots, hashes and backslashes in identifiers/paths.
`as_position()` supplies a typed position-property facade. References retain their
collection and target entity weakly: the application must keep the collection
alive. Collection owners are weak too. Each resolution verifies current membership,
including replacement under event suspension, and discards retired target
subscriptions. Source notification timing is retained for normal operations.
Factory errors leave existing values/subscriptions intact, and reentrant setters
cannot reinstall a retired slot's child subscription. These ownership/reentrancy
guarantees are deliberate native improvements over the pinned source.

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

`tests/data_sources_positions.rs` adds **412 pinned scenarios** across all ten
position/motion modules. They cover TEME fallback, loaded IAU data, Moon rotation,
derivative interpolation, packed inputs, intervals, mixed frames, scaling and
velocity endpoint/stationary behavior. Eight additional native lifecycle tests
exercise reentrant callbacks/arrays, composite rewiring, subscription cleanup,
coherent clocks, transform errors, nested motion and delayed Earth data. The
Chromium harness also evaluates every position/motion class with omitted times.

`tests/data_sources_entities.rs` adds **126 pinned scenarios** across the six
entity modules: descriptors, object-key ordering, component factories and merges,
show hierarchy, availability, model/height matrices, scratch reuse, collection
coalescing/reentrancy and escaped references. **14 native lifecycle tests** cover
payload nesting/drop, descriptor renewal/failure, child removal, parent ownership,
old-value lifetimes and suspended ID replacement. The Chromium harness checks
browser clocks, batching, hierarchy, reference evaluation and listener cleanup.
DataSources now has **744 pinned scenarios** overall.

Next, implement CompositeEntityCollection, concrete graphics definitions,
DataSourceClock/data-source collections and the loaders in dependency order. The largest document modules are CzmlDataSource
(5,142 upstream lines), KmlDataSource (4,249), GeoJsonDataSource (1,064) and
GpxDataSource (1,018). Their property/entity dependencies must exist before their
loader behavior can be completed and validated.
