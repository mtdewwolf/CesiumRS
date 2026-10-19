//! CesiumJS credit identities and attribution metadata.
//!
//! Raw HTML is data. A host renderer must sanitize it before inserting it into
//! a DOM, create the inline `cesium-credit-wrapper`, and set link targets to
//! `_blank`. The renderer owns element caching and lifetime.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Debug)]
pub struct Credit {
    id: u64,
    html: String,
    pub show_on_screen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribution {
    pub html: String,
    pub collapsible: Option<bool>,
}

pub trait CreditRenderer {
    type Element;
    fn element(&mut self, credit: &Credit) -> Self::Element;
}

impl Credit {
    pub fn new(html: impl Into<String>, show_on_screen: bool) -> Self {
        static IDS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
        let html = html.into();
        let mut ids = IDS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        let next = ids.len() as u64;
        let id = *ids.entry(html.clone()).or_insert(next);
        Self { id, html, show_on_screen }
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn html(&self) -> &str {
        &self.html
    }
    pub fn is_ion(&self) -> bool {
        self.html.contains("ion-credit.png")
    }
    pub fn get_ion_credit(attribution: &Attribution) -> Self {
        Self::new(attribution.html.clone(), attribution.collapsible == Some(false))
    }
    pub fn element<H: CreditRenderer>(&self, renderer: &mut H) -> H::Element {
        renderer.element(self)
    }
}

impl PartialEq for Credit {
    fn eq(&self, right: &Self) -> bool {
        self.id == right.id && self.show_on_screen == right.show_on_screen
    }
}
impl Eq for Credit {}
