//! CesiumJS `TileAvailability`: quadtree-indexed availability rectangles.
//! Add root tiles before finer levels. Earlier ranges are not replayed when
//! roots are added later, matching Cesium's insertion-order behavior.

use super::cartographic::Cartographic;
use super::math::{js_max, js_min, PI};
use super::rectangle::Rectangle;
use super::tiling_scheme::TilingScheme;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
struct AvailableRectangle {
    level: u32,
    extent: Rectangle,
}

#[derive(Debug)]
struct Node {
    level: u32,
    x: u32,
    y: u32,
    extent: Rectangle,
    rectangles: Vec<AvailableRectangle>,
    // NW, NE, SW, SE, in Cesium's traversal order.
    children: [Option<Box<Node>>; 4],
}

impl Node {
    fn new(scheme: &dyn TilingScheme, level: u32, x: u32, y: u32) -> Self {
        Self {
            level,
            x,
            y,
            extent: scheme.tile_xy_to_rectangle(x as f64, y as f64, level),
            rectangles: Vec::new(),
            children: Default::default(),
        }
    }

    fn insert(&mut self, scheme: &dyn TilingScheme, max_depth: u32, rectangle: AvailableRectangle) -> Option<()> {
        if self.level < max_depth {
            for quadrant in 0..4 {
                if self.children[quadrant].is_none() {
                    let x = self.x.checked_mul(2)?.checked_add((quadrant % 2) as u32)?;
                    let y = self.y.checked_mul(2)?.checked_add((quadrant / 2) as u32)?;
                    self.children[quadrant] = Some(Box::new(Node::new(scheme, self.level.checked_add(1)?, x, y)));
                }
                let child = self.children[quadrant].as_mut()?;
                if contains_rectangle(&child.extent, &rectangle.extent) {
                    return child.insert(scheme, max_depth, rectangle);
                }
            }
        }
        let i = self.rectangles.partition_point(|r| r.level <= rectangle.level);
        self.rectangles.insert(i, rectangle);
        Some(())
    }

    fn maximum_at(&self, position: &Cartographic) -> u32 {
        let mut level = 0;
        for r in self.rectangles.iter().rev() {
            if r.level <= level {
                break;
            }
            if contains_position(&r.extent, position) {
                level = r.level;
            }
        }
        // Cesium's `nw + ne + sw + se` becomes NaN when a child is
        // unallocated (undefined). It then follows only the first matching
        // quadrant, even on a shared boundary. Once all four children exist,
        // it visits every matching quadrant. Preserve this observable quirk.
        let visit_all = self.children.iter().all(Option::is_some);
        for child in self.children.iter().flatten() {
            if contains_position(&child.extent, position) {
                level = level.max(child.maximum_at(position));
                if !visit_all {
                    break;
                }
            }
        }
        level
    }

    fn update_coverage(&self, remaining: &mut BTreeMap<u32, Vec<Rectangle>>, query: &[Rectangle]) {
        if !query.iter().any(|r| overlaps(&self.extent, r)) {
            return;
        }
        for r in &self.rectangles {
            let previous = remaining.entry(r.level).or_insert_with(|| query.to_vec());
            *previous = subtract_rectangle(previous, &r.extent);
        }
        for child in self.children.iter().flatten() {
            child.update_coverage(remaining, query);
        }
    }
}

pub struct TileAvailability {
    tiling_scheme: Box<dyn TilingScheme>,
    maximum_level: u32,
    roots: Vec<Node>,
}

impl std::fmt::Debug for TileAvailability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileAvailability")
            .field("maximum_level", &self.maximum_level)
            .field("roots", &self.roots)
            .finish_non_exhaustive()
    }
}

impl TileAvailability {
    pub fn new(scheme: impl TilingScheme + 'static, maximum_level: u32) -> Self {
        Self::from_boxed(Box::new(scheme), maximum_level)
    }

    pub fn from_boxed(tiling_scheme: Box<dyn TilingScheme>, maximum_level: u32) -> Self {
        Self { tiling_scheme, maximum_level, roots: Vec::new() }
    }

    pub fn tiling_scheme(&self) -> &dyn TilingScheme {
        &*self.tiling_scheme
    }
    pub fn maximum_level(&self) -> u32 {
        self.maximum_level
    }

    /// Returns None for inverted ranges or coordinates that overflow the
    /// quadtree. Availability beyond maximum_level is stored as in Cesium;
    /// maximum_level limits tree depth and child-mask queries.
    pub fn add_available_tile_range(
        &mut self,
        level: u32,
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
    ) -> Option<()> {
        if start_x > end_x || start_y > end_y {
            return None;
        }
        let scheme = &*self.tiling_scheme;
        if level == 0 {
            for y in start_y..=end_y {
                for x in start_x..=end_x {
                    if !self.roots.iter().any(|r| r.x == x && r.y == y) {
                        self.roots.push(Node::new(scheme, 0, x, y));
                    }
                }
            }
        }
        let start = scheme.tile_xy_to_rectangle(start_x as f64, start_y as f64, level);
        let end = scheme.tile_xy_to_rectangle(end_x as f64, end_y as f64, level);
        let r = AvailableRectangle { level, extent: Rectangle::new(start.west, end.south, end.east, start.north) };
        for root in &mut self.roots {
            if overlaps(&root.extent, &r.extent) {
                root.insert(scheme, self.maximum_level, r)?;
            }
        }
        Some(())
    }

    /// -1 if no root contains the position. Longitude is tested literally,
    /// without Rectangle::contains's antimeridian normalization.
    pub fn compute_maximum_level_at_position(&self, position: &Cartographic) -> i64 {
        self.roots.iter().find(|r| contains_position(&r.extent, position)).map_or(-1, |r| r.maximum_at(position) as i64)
    }

    pub fn compute_best_available_level_over_rectangle(&self, rectangle: &Rectangle) -> u32 {
        let query = if rectangle.east < rectangle.west {
            vec![
                Rectangle::new(-PI, rectangle.south, rectangle.east, rectangle.north),
                Rectangle::new(rectangle.west, rectangle.south, PI, rectangle.north),
            ]
        } else {
            vec![*rectangle]
        };
        let mut remaining = BTreeMap::new();
        for root in &self.roots {
            root.update_coverage(&mut remaining, &query);
        }
        remaining.iter().rev().find_map(|(&level, r)| r.is_empty().then_some(level)).unwrap_or(0)
    }

    pub fn is_tile_available(&self, level: u32, x: u32, y: u32) -> bool {
        let extent = self.tiling_scheme.tile_xy_to_rectangle(x as f64, y as f64, level);
        self.compute_maximum_level_at_position(&extent.center()) >= level as i64
    }

    /// SW=1, SE=2, NW=4, NE=8. Cesium returns zero when child_level is
    /// equal to maximum_level as well as when it exceeds it.
    pub fn compute_child_mask_for_tile(&self, level: u32, x: u32, y: u32) -> u8 {
        let Some(child_level) = level.checked_add(1).filter(|l| *l < self.maximum_level) else {
            return 0;
        };
        let (Some(x), Some(y)) = (x.checked_mul(2), y.checked_mul(2)) else {
            return 0;
        };
        let (Some(east), Some(south)) = (x.checked_add(1), y.checked_add(1)) else {
            return 0;
        };
        u8::from(self.is_tile_available(child_level, x, south))
            | (u8::from(self.is_tile_available(child_level, east, south)) << 1)
            | (u8::from(self.is_tile_available(child_level, x, y)) << 2)
            | (u8::from(self.is_tile_available(child_level, east, y)) << 3)
    }
}

fn overlaps(a: &Rectangle, b: &Rectangle) -> bool {
    js_max(a.south, b.south) < js_min(a.north, b.north) && js_max(a.west, b.west) < js_min(a.east, b.east)
}

fn contains_rectangle(a: &Rectangle, b: &Rectangle) -> bool {
    b.west >= a.west && b.east <= a.east && b.south >= a.south && b.north <= a.north
}

fn contains_position(a: &Rectangle, p: &Cartographic) -> bool {
    p.longitude >= a.west && p.longitude <= a.east && p.latitude >= a.south && p.latitude <= a.north
}

fn subtract_rectangle(rectangles: &[Rectangle], cut: &Rectangle) -> Vec<Rectangle> {
    let mut result = Vec::new();
    for r in rectangles {
        if !overlaps(r, cut) {
            result.push(*r);
            continue;
        }
        if r.west < cut.west {
            result.push(Rectangle::new(r.west, r.south, cut.west, r.north));
        }
        if r.east > cut.east {
            result.push(Rectangle::new(cut.east, r.south, r.east, r.north));
        }
        if r.south < cut.south {
            result.push(Rectangle::new(js_max(cut.west, r.west), r.south, js_min(cut.east, r.east), cut.south));
        }
        if r.north > cut.north {
            result.push(Rectangle::new(js_max(cut.west, r.west), cut.north, js_min(cut.east, r.east), r.north));
        }
    }
    result
}
