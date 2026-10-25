//! Time-varying properties, typed entity components, collections and references.
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

pub mod position_property;
pub use position_property::{FrameConversion, FrameTransform, PositionContext, PositionProperty, PositionPropertyRef};
pub mod constant_position_property;
pub use constant_position_property::ConstantPositionProperty;
pub mod callback_position_property;
pub use callback_position_property::CallbackPositionProperty;
pub mod sampled_position_property;
pub use sampled_position_property::SampledPositionProperty;
pub mod time_interval_collection_position_property;
pub use time_interval_collection_position_property::TimeIntervalCollectionPositionProperty;

pub mod composite_position_property;
mod interval_subscriptions;
pub use composite_position_property::CompositePositionProperty;
pub mod position_property_array;
pub use position_property_array::PositionPropertyArray;
pub mod scaled_position_property;
pub use scaled_position_property::ScaledPositionProperty;
pub mod velocity_vector_property;
pub use velocity_vector_property::VelocityVectorProperty;
pub mod velocity_orientation_property;
pub use velocity_orientation_property::VelocityOrientationProperty;
pub mod change_event;
pub use change_event::{ChangeEvent, ChangeListener, WeakChangeEvent};
pub mod dynamic_value;
pub use dynamic_value::{BagValues, ComponentValue, DynamicValue};
pub mod entity_component;
pub use entity_component::{ComponentRef, EntityComponent, PositionComponent, PropertyComponent, PropertyInput};
pub mod create_property_descriptor;
pub use create_property_descriptor::{
    create_property_descriptor, ComponentFactory, PropertyChange, PropertyDescriptor,
};
pub mod create_raw_property_descriptor;
pub use create_raw_property_descriptor::create_raw_property_descriptor;
pub mod property_bag;
pub use property_bag::PropertyBag;

pub mod entity;
pub use entity::{
    Availability, Entity, EntityChange, EntityOptions, EntityRef, EntityTypeRegistry, HeightReferenceOptions,
};
pub mod entity_collection;
pub use entity_collection::{CollectionChange, EntityCollection, EntityCollectionOptions, EntitySource};
pub mod reference_property;
pub use reference_property::ReferenceProperty;
pub mod composite_entity_collection;
pub use composite_entity_collection::{CompositeEntityCollection, EntitySourceRef};
