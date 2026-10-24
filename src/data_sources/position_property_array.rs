use super::{
    position_property::*, property::write_result, DefinitionChanged, Property, PropertyListener, Subscription,
};
use crate::core::*;
use std::{any::Any, cell::RefCell, rc::Rc};
pub struct PositionPropertyArray {
    value: RefCell<Option<Vec<PositionPropertyRef>>>,
    subscriptions: RefCell<Vec<Subscription>>,
    bridge: PropertyListener,
    changed: DefinitionChanged,
    frame: ReferenceFrame,
    context: PositionContext,
}
impl PositionPropertyArray {
    pub fn new(value: Option<Vec<PositionPropertyRef>>, frame: ReferenceFrame) -> Self {
        Self::with_context(value, frame, PositionContext::default())
    }
    pub fn with_context(
        value: Option<Vec<PositionPropertyRef>>,
        frame: ReferenceFrame,
        context: PositionContext,
    ) -> Self {
        let changed = DefinitionChanged::new();
        let weak = changed.downgrade();
        let p = Self {
            value: RefCell::new(None),
            subscriptions: RefCell::new(vec![]),
            bridge: Rc::new(move |_| {
                if let Some(event) = weak.upgrade() {
                    event.raise();
                }
            }),
            changed,
            frame,
            context,
        };
        p.set_value(value);
        p
    }
    pub fn reference_frame(&self) -> ReferenceFrame {
        self.frame
    }
    pub fn set_value(&self, value: Option<Vec<PositionPropertyRef>>) {
        let old = std::mem::take(&mut *self.subscriptions.borrow_mut());
        drop(old);
        *self.value.borrow_mut() = value;
        let properties = self.value.borrow().clone();
        let mut subscribed = Vec::<DefinitionChanged>::new();
        let mut subscriptions = vec![];
        for p in properties.iter().flatten() {
            let event = p.definition_changed();
            if !subscribed.iter().any(|e| e.ptr_eq(event)) {
                subscriptions.push(event.subscribe(self.bridge.clone()));
                subscribed.push(event.clone());
            }
        }
        *self.subscriptions.borrow_mut() = subscriptions;
        self.changed.raise();
    }
    pub fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        mut result: Option<&mut Vec<Cartesian3>>,
    ) -> Result<Option<Vec<Cartesian3>>, RuntimeError> {
        let Some(properties) = self.value.borrow().clone() else { return Ok(None) };
        let mut values = Vec::with_capacity(properties.len());
        for (i, p) in properties.iter().enumerate() {
            if let Some(value) =
                p.get_value_in_reference_frame(time, frame, result.as_deref_mut().and_then(|r| r.get_mut(i)))?
            {
                values.push(value);
            }
        }
        Ok(write_result(Some(values), result))
    }
}
impl Property<Vec<Cartesian3>> for PositionPropertyArray {
    fn is_constant(&self) -> bool {
        let properties = self.value.borrow().clone();
        properties.iter().flatten().all(|p| p.is_constant())
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Vec<Cartesian3>>,
    ) -> Result<Option<Vec<Cartesian3>>, RuntimeError> {
        self.get_value_in_reference_frame(&self.context.time(time), ReferenceFrame::Fixed, result)
    }
    fn equals(&self, other: &dyn Property<Vec<Cartesian3>>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if std::ptr::eq(self, o) {
            return Ok(true);
        }
        if self.frame != o.frame {
            return Ok(false);
        }
        let (a, b) = (self.value.borrow().clone(), o.value.borrow().clone());
        match (a, b) {
            (None, None) => Ok(true),
            (Some(a), Some(b)) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (a, b) in a.iter().zip(&b) {
                    if !position_equals(Some(a), Some(b))? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for PositionPropertyArray {
    fn default() -> Self {
        Self::new(None, ReferenceFrame::Fixed)
    }
}
