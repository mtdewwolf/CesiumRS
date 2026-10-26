//! CesiumJS `CreditDisplay`: responsible for managing, deduplicating, and sorting
//! screen and lightbox attributions and logo credits per frame.

use crate::core::Credit;
use std::collections::HashMap;

/// An element in the credit display with an occurrence counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditDisplayElement {
    pub credit: Credit,
    pub count: usize,
}

impl CreditDisplayElement {
    pub fn new(credit: Credit, count: usize) -> Self {
        Self { credit, count }
    }
}

/// The credit display collects attributions from primitives and data sources
/// and formats them for on-screen or popup (lightbox) presentation.
#[derive(Clone, Debug)]
pub struct CreditDisplay {
    delimiter: String,
    cesium_credit: Credit,
    current_cesium_credit: Credit,
    default_credit: Option<Credit>,
    static_credits: Vec<Credit>,
    screen_credits: HashMap<u64, CreditDisplayElement>,
    lightbox_credits: HashMap<u64, CreditDisplayElement>,
    sorted_screen_credits: Vec<CreditDisplayElement>,
    sorted_lightbox_credits: Vec<CreditDisplayElement>,
    destroyed: bool,
}

impl Default for CreditDisplay {
    fn default() -> Self {
        Self::new(None)
    }
}

impl CreditDisplay {
    pub fn default_cesium_credit() -> Credit {
        Credit::new(
            r#"<a href="https://cesium.com/" target="_blank"><img src="Assets/Images/ion-credit.png" style="vertical-align: -7px" title="Cesium ion"/></a>"#,
            true,
        )
    }

    pub fn new(delimiter: Option<String>) -> Self {
        let cesium_credit = Self::default_cesium_credit();
        Self {
            delimiter: delimiter.unwrap_or_else(|| "•".into()),
            cesium_credit: cesium_credit.clone(),
            current_cesium_credit: cesium_credit,
            default_credit: None,
            static_credits: Vec::new(),
            screen_credits: HashMap::new(),
            lightbox_credits: HashMap::new(),
            sorted_screen_credits: Vec::new(),
            sorted_lightbox_credits: Vec::new(),
            destroyed: false,
        }
    }

    pub fn delimiter(&self) -> &str {
        &self.delimiter
    }

    pub fn set_delimiter(&mut self, delimiter: impl Into<String>) {
        self.delimiter = delimiter.into();
    }

    pub fn cesium_credit(&self) -> &Credit {
        &self.cesium_credit
    }

    pub fn set_cesium_credit(&mut self, credit: Credit) {
        self.cesium_credit = credit.clone();
        self.current_cesium_credit = credit;
    }

    pub fn static_credits(&self) -> &[Credit] {
        &self.static_credits
    }

    pub fn add_static_credit(&mut self, credit: Credit) {
        if !self.static_credits.iter().any(|c| c == &credit) {
            self.static_credits.push(credit);
        }
    }

    pub fn remove_static_credit(&mut self, credit: &Credit) {
        if let Some(pos) = self.static_credits.iter().position(|c| c == credit) {
            self.static_credits.remove(pos);
        }
    }

    pub fn add_credit_to_next_frame(&mut self, credit: Credit) {
        if credit.is_ion() {
            if self.default_credit.is_none() {
                self.default_credit = Some(Self::default_cesium_credit());
            }
            if let Some(def) = &self.default_credit {
                self.current_cesium_credit = def.clone();
            }
            return;
        }

        let id = credit.id();
        let target_map = if credit.show_on_screen { &mut self.screen_credits } else { &mut self.lightbox_credits };

        target_map
            .entry(id)
            .and_modify(|elem| elem.count = elem.count.saturating_add(1))
            .or_insert_with(|| CreditDisplayElement::new(credit, 1));
    }

    pub fn begin_frame(&mut self) {
        self.screen_credits.clear();
        self.lightbox_credits.clear();

        for static_credit in &self.static_credits {
            if static_credit.is_ion() && self.cesium_credit == Self::default_cesium_credit() {
                continue;
            }
            let id = static_credit.id();
            let target_map =
                if static_credit.show_on_screen { &mut self.screen_credits } else { &mut self.lightbox_credits };
            target_map.insert(id, CreditDisplayElement::new(static_credit.clone(), usize::MAX));
        }

        self.current_cesium_credit = self.cesium_credit.clone();
    }

    pub fn end_frame(&mut self) {
        let mut screen: Vec<CreditDisplayElement> = self.screen_credits.drain().map(|(_, v)| v).collect();
        screen.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.credit.id().cmp(&b.credit.id())));
        self.sorted_screen_credits = screen;

        let mut lightbox: Vec<CreditDisplayElement> = self.lightbox_credits.drain().map(|(_, v)| v).collect();
        lightbox.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.credit.id().cmp(&b.credit.id())));
        self.sorted_lightbox_credits = lightbox;
    }

    pub fn current_screen_credits(&self) -> &[CreditDisplayElement] {
        &self.sorted_screen_credits
    }

    pub fn current_lightbox_credits(&self) -> &[CreditDisplayElement] {
        &self.sorted_lightbox_credits
    }

    pub fn current_cesium_credit(&self) -> &Credit {
        &self.current_cesium_credit
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        self.destroyed = true;
    }
}
