//! CesiumJS `RectangleCollisionChecker`, including rbush 4's insertion,
//! splitting, traversal and ID-based removal order. Edges count as collisions;
//! longitude intervals are used literally, without antimeridian splitting.
use super::{
    math::{js_max, js_min},
    Rectangle,
};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug)]
struct Bounds {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
}
impl Bounds {
    fn empty() -> Self {
        Self { west: f64::INFINITY, south: f64::INFINITY, east: f64::NEG_INFINITY, north: f64::NEG_INFINITY }
    }
    fn extend(&mut self, b: Self) {
        self.west = js_min(self.west, b.west);
        self.south = js_min(self.south, b.south);
        self.east = js_max(self.east, b.east);
        self.north = js_max(self.north, b.north);
    }
    fn contains(self, b: Self) -> bool {
        self.west <= b.west && self.south <= b.south && b.east <= self.east && b.north <= self.north
    }
    fn intersects(self, b: Self) -> bool {
        b.west <= self.east && b.south <= self.north && b.east >= self.west && b.north >= self.south
    }
    fn area(self) -> f64 {
        (self.east - self.west) * (self.north - self.south)
    }
    fn margin(self) -> f64 {
        self.east - self.west + self.north - self.south
    }
    fn intersection_area(self, b: Self) -> f64 {
        js_max(0.0, js_min(self.east, b.east) - js_max(self.west, b.west))
            * js_max(0.0, js_min(self.north, b.north) - js_max(self.south, b.south))
    }
}
impl From<Rectangle> for Bounds {
    fn from(r: Rectangle) -> Self {
        Self { west: r.west, south: r.south, east: r.east, north: r.north }
    }
}
#[derive(Clone, Debug)]
enum Entry {
    Item(String, Bounds),
    Node(Node),
}
impl Entry {
    fn bounds(&self) -> Bounds {
        match self {
            Self::Item(_, b) => *b,
            Self::Node(n) => n.bounds,
        }
    }
}
#[derive(Clone, Debug)]
struct Node {
    bounds: Bounds,
    leaf: bool,
    children: Vec<Entry>,
}
impl Node {
    fn new(leaf: bool, children: Vec<Entry>) -> Self {
        let bounds = Self::bounds_of(&children);
        Self { bounds, leaf, children }
    }
    fn bounds_of(entries: &[Entry]) -> Bounds {
        let mut b = Bounds::empty();
        for e in entries {
            b.extend(e.bounds());
        }
        b
    }
    fn recalculate(&mut self) {
        self.bounds = Self::bounds_of(&self.children);
    }
    fn insert(&mut self, entry: Entry) -> Option<Node> {
        let bbox = entry.bounds();
        if self.leaf {
            self.children.push(entry);
        } else {
            let (mut min_area, mut min_enlargement) = (f64::INFINITY, f64::INFINITY);
            let mut target = 0;
            for (i, child) in self.children.iter().enumerate() {
                let b = child.bounds();
                let area = b.area();
                let mut enlarged = b;
                enlarged.extend(bbox);
                let enlargement = enlarged.area() - area;
                if enlargement < min_enlargement {
                    min_enlargement = enlargement;
                    min_area = js_min(area, min_area);
                    target = i;
                } else if enlargement == min_enlargement && area < min_area {
                    min_area = area;
                    target = i;
                }
            }
            let Entry::Node(child) = &mut self.children[target] else { unreachable!() };
            if let Some(sibling) = child.insert(entry) {
                self.children.push(Entry::Node(sibling));
            }
        }
        self.bounds.extend(bbox);
        if self.children.len() > 9 {
            Some(self.split())
        } else {
            None
        }
    }
    fn sort(&mut self, x: bool) {
        self.children.sort_by(|a, b| {
            let (a, b) = (a.bounds(), b.bounds());
            let difference = if x { a.west - b.west } else { a.south - b.south };
            difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
        });
    }
    fn distribution_margin(&mut self, x: bool) -> f64 {
        self.sort(x);
        let m = 4;
        let count = self.children.len();
        let mut left = Self::bounds_of(&self.children[..m]);
        let mut right = Self::bounds_of(&self.children[count - m..]);
        let mut margin = left.margin() + right.margin();
        for i in m..count - m {
            left.extend(self.children[i].bounds());
            margin += left.margin();
        }
        for i in (m..count - m).rev() {
            right.extend(self.children[i].bounds());
            margin += right.margin();
        }
        margin
    }
    fn split(&mut self) -> Self {
        let x_margin = self.distribution_margin(true);
        let y_margin = self.distribution_margin(false);
        if x_margin < y_margin {
            self.sort(true);
        }
        let count = self.children.len();
        let mut index = count - 4;
        let (mut min_overlap, mut min_area) = (f64::INFINITY, f64::INFINITY);
        for i in 4..=count - 4 {
            let a = Self::bounds_of(&self.children[..i]);
            let b = Self::bounds_of(&self.children[i..]);
            let overlap = a.intersection_area(b);
            let area = a.area() + b.area();
            if overlap < min_overlap {
                min_overlap = overlap;
                index = i;
                min_area = js_min(area, min_area);
            } else if overlap == min_overlap && area < min_area {
                min_area = area;
                index = i;
            }
        }
        let sibling = Self::new(self.leaf, self.children.split_off(index));
        self.recalculate();
        sibling
    }
    fn remove(&mut self, id: &str, bbox: Bounds) -> bool {
        if self.leaf {
            if let Some(i) = self.children.iter().position(|e| matches!(e,Entry::Item(key,_) if key==id)) {
                self.children.remove(i);
                self.recalculate();
                return true;
            }
        } else if self.bounds.contains(bbox) {
            for i in 0..self.children.len() {
                let Entry::Node(child) = &mut self.children[i] else { unreachable!() };
                if child.remove(id, bbox) {
                    if child.children.is_empty() {
                        self.children.remove(i);
                    }
                    self.recalculate();
                    return true;
                }
            }
        }
        false
    }
    fn collides(&self, bbox: Bounds) -> bool {
        if !self.bounds.intersects(bbox) {
            return false;
        }
        self.children.iter().any(|e| {
            let b = e.bounds();
            b.intersects(bbox)
                && (self.leaf || bbox.contains(b) || matches!(e,Entry::Node(child) if child.collides(bbox)))
        })
    }
}
#[derive(Clone, Debug)]
pub struct RectangleCollisionChecker {
    root: Node,
}
impl Default for RectangleCollisionChecker {
    fn default() -> Self {
        Self { root: Node::new(true, Vec::new()) }
    }
}
impl RectangleCollisionChecker {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, id: impl Into<String>, rectangle: Rectangle) {
        if let Some(sibling) = self.root.insert(Entry::Item(id.into(), rectangle.into())) {
            let old = std::mem::replace(&mut self.root, Node::new(true, Vec::new()));
            self.root = Node::new(false, vec![Entry::Node(old), Entry::Node(sibling)]);
        }
    }
    pub fn remove(&mut self, id: &str, rectangle: &Rectangle) {
        self.root.remove(id, (*rectangle).into());
        if self.root.children.is_empty() {
            self.root = Node::new(true, Vec::new());
        }
    }
    pub fn collides(&self, rectangle: &Rectangle) -> bool {
        self.root.collides((*rectangle).into())
    }
}
