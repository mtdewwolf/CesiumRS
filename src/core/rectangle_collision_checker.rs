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
fn combined(bounds: impl IntoIterator<Item = Bounds>) -> Bounds {
    let mut total = Bounds::empty();
    for b in bounds {
        total.extend(b);
    }
    total
}
fn sort_by_axis<T>(entries: &mut [T], x: bool, bounds: impl Fn(&T) -> Bounds) {
    entries.sort_by(|a, b| {
        let (a, b) = (bounds(a), bounds(b));
        let difference = if x { a.west - b.west } else { a.south - b.south };
        difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
    });
}
fn distribution_margin<T>(entries: &mut [T], x: bool, bounds: impl Fn(&T) -> Bounds) -> f64 {
    sort_by_axis(entries, x, &bounds);
    let m = 4;
    let count = entries.len();
    let mut left = combined(entries[..m].iter().map(&bounds));
    let mut right = combined(entries[count - m..].iter().map(&bounds));
    let mut margin = left.margin() + right.margin();
    for entry in entries.iter().take(count - m).skip(m) {
        left.extend(bounds(entry));
        margin += left.margin();
    }
    for entry in entries.iter().take(count - m).skip(m).rev() {
        right.extend(bounds(entry));
        margin += right.margin();
    }
    margin
}
fn split_children<T>(children: &mut Vec<T>, bounds: impl Fn(&T) -> Bounds) -> Vec<T> {
    let x_margin = distribution_margin(children, true, &bounds);
    let y_margin = distribution_margin(children, false, &bounds);
    if x_margin < y_margin {
        sort_by_axis(children, true, &bounds);
    }
    let count = children.len();
    let mut index = count - 4;
    let (mut min_overlap, mut min_area) = (f64::INFINITY, f64::INFINITY);
    for i in 4..=count - 4 {
        let a = combined(children[..i].iter().map(&bounds));
        let b = combined(children[i..].iter().map(&bounds));
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
    children.split_off(index)
}
/// Leaf nodes store rectangles. Branch nodes store only child nodes, so the
/// old `unreachable!` arms cannot be represented.
#[derive(Clone, Debug)]
enum Node {
    Leaf { bounds: Bounds, children: Vec<(String, Bounds)> },
    Branch { bounds: Bounds, children: Vec<Node> },
}
impl Node {
    fn empty_leaf() -> Self {
        Self::Leaf { bounds: Bounds::empty(), children: Vec::new() }
    }
    fn branch(children: Vec<Node>) -> Self {
        let bounds = combined(children.iter().map(Self::bounds));
        Self::Branch { bounds, children }
    }
    fn bounds(&self) -> Bounds {
        match self {
            Self::Leaf { bounds, .. } | Self::Branch { bounds, .. } => *bounds,
        }
    }
    fn is_empty(&self) -> bool {
        match self {
            Self::Leaf { children, .. } => children.is_empty(),
            Self::Branch { children, .. } => children.is_empty(),
        }
    }
    fn insert(&mut self, id: String, bbox: Bounds) -> Option<Node> {
        let split = match self {
            Self::Leaf { bounds, children } => {
                children.push((id, bbox));
                bounds.extend(bbox);
                children.len() > 9
            }
            Self::Branch { bounds, children } => {
                let (mut min_area, mut min_enlargement) = (f64::INFINITY, f64::INFINITY);
                let mut target = 0;
                for (i, child) in children.iter().enumerate() {
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
                let child = children.get_mut(target)?;
                if let Some(sibling) = child.insert(id, bbox) {
                    children.push(sibling);
                }
                bounds.extend(bbox);
                children.len() > 9
            }
        };
        split.then(|| self.split())
    }
    fn split(&mut self) -> Self {
        match self {
            Self::Leaf { bounds, children } => {
                let sibling = split_children(children, |(_, b)| *b);
                *bounds = combined(children.iter().map(|(_, b)| *b));
                Self::Leaf { bounds: combined(sibling.iter().map(|(_, b)| *b)), children: sibling }
            }
            Self::Branch { bounds, children } => {
                let sibling = split_children(children, Node::bounds);
                *bounds = combined(children.iter().map(Node::bounds));
                Self::branch(sibling)
            }
        }
    }
    fn remove(&mut self, id: &str, bbox: Bounds) -> bool {
        match self {
            Self::Leaf { bounds, children } => {
                let Some(i) = children.iter().position(|(key, _)| key == id) else { return false };
                children.remove(i);
                *bounds = combined(children.iter().map(|(_, b)| *b));
                true
            }
            Self::Branch { bounds, children } => {
                if !bounds.contains(bbox) {
                    return false;
                }
                for i in 0..children.len() {
                    if children[i].remove(id, bbox) {
                        if children[i].is_empty() {
                            children.remove(i);
                        }
                        *bounds = combined(children.iter().map(Node::bounds));
                        return true;
                    }
                }
                false
            }
        }
    }
    fn collides(&self, bbox: Bounds) -> bool {
        if !self.bounds().intersects(bbox) {
            return false;
        }
        match self {
            Self::Leaf { children, .. } => children.iter().any(|(_, b)| b.intersects(bbox)),
            Self::Branch { children, .. } => children.iter().any(|child| {
                let b = child.bounds();
                b.intersects(bbox) && (bbox.contains(b) || child.collides(bbox))
            }),
        }
    }
}
#[derive(Clone, Debug)]
pub struct RectangleCollisionChecker {
    root: Node,
}
impl Default for RectangleCollisionChecker {
    fn default() -> Self {
        Self { root: Node::empty_leaf() }
    }
}
impl RectangleCollisionChecker {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, id: impl Into<String>, rectangle: Rectangle) {
        if let Some(sibling) = self.root.insert(id.into(), rectangle.into()) {
            let old = std::mem::replace(&mut self.root, Node::empty_leaf());
            self.root = Node::branch(vec![old, sibling]);
        }
    }
    pub fn remove(&mut self, id: &str, rectangle: &Rectangle) {
        self.root.remove(id, (*rectangle).into());
        if self.root.is_empty() {
            self.root = Node::empty_leaf();
        }
    }
    pub fn collides(&self, rectangle: &Rectangle) -> bool {
        self.root.collides((*rectangle).into())
    }
}

#[cfg(test)]
mod tests {
    use super::RectangleCollisionChecker;
    use crate::core::Rectangle;

    fn overlaps(a: Rectangle, b: Rectangle) -> bool {
        b.west <= a.east && b.south <= a.north && b.east >= a.west && b.north >= a.south
    }

    #[test]
    fn split_tree_matches_a_linear_scan() {
        let mut tree = RectangleCollisionChecker::new();
        let mut stored = Vec::new();
        for i in 0..24 {
            let west = i as f64 * 0.05;
            let rectangle = Rectangle::new(west, 0.0, west + 0.2, 0.3);
            let id = format!("r{i}");
            tree.insert(id.clone(), rectangle);
            stored.push((id, rectangle));
        }
        let probes = [
            Rectangle::new(0.0, 0.0, 0.01, 0.01),
            Rectangle::new(0.4, 0.1, 0.55, 0.2),
            Rectangle::new(2.0, 2.0, 2.1, 2.1),
            Rectangle::new(-1.0, -1.0, 0.0, 0.0),
        ];
        let check = |tree: &RectangleCollisionChecker, stored: &[(String, Rectangle)]| {
            for probe in probes {
                let linear = stored.iter().any(|(_, rectangle)| overlaps(*rectangle, probe));
                assert_eq!(tree.collides(&probe), linear, "{probe:?}");
            }
        };
        check(&tree, &stored);
        let (id, rectangle) = stored.remove(5);
        tree.remove(&id, &rectangle);
        check(&tree, &stored);
        let (id, rectangle) = stored.remove(0);
        tree.remove(&id, &rectangle);
        check(&tree, &stored);
    }
}
