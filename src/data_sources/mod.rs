//! Typed, time-varying entity properties and their definition-change events.
pub mod events;
pub use events::{DefinitionChanged, PropertyListener, Subscription, WeakDefinitionChanged};
pub mod property;
pub use property::{Property, PropertyClock, PropertyRef};
pub mod value;
pub use value::{CloneablePropertyValue, JsonReference, PropertyValue};
pub mod constant_property;
pub use constant_property::ConstantProperty;
pub mod callback_property;
pub use callback_property::{CallbackProperty, PropertyCallback};

pub mod property_array;
pub use property_array::PropertyArray;
pub mod sampled_property;
pub use sampled_property::{Interpolation, PackedSampleValue, SampleType, SampledProperty, SampledValue};

pub mod intervals;
pub use intervals::PropertyIntervals;
pub mod time_interval_collection_property;
pub use time_interval_collection_property::TimeIntervalCollectionProperty;
pub mod composite_property;
pub use composite_property::CompositeProperty;
