//! A port of earcut 3.2.4 (Mapbox, ISC license), the polygon triangulator
//! CesiumJS's `PolygonPipeline` uses. Triangles come out in the same order as
//! the JavaScript library's, so geometries built on them match CesiumJS.
//!
//! Nodes live in an arena and link by index; the module-level state of the
//! JavaScript (Steiner points, the hole-bridge block index, sort buffers) is
//! held in a per-call [`Earcut`] value.

use std::collections::HashSet;

type Id = usize;

#[derive(Clone, Copy, Debug)]
struct Node {
    /// Vertex index in the coordinates array.
    i: usize,
    x: f64,
    y: f64,
    prev: Id,
    next: Id,
    /// Z-order value; during hole elimination, the owning block's index.
    z: i32,
    prev_z: Option<Id>,
    next_z: Option<Id>,
}

/// Edges per block of the hole-bridge index.
const K: usize = 16;

struct Earcut {
    nodes: Vec<Node>,
    steiners: HashSet<Id>,
    filtered_out: bool,
    index_active: bool,
    block_bbox: Vec<f64>,
    num_blocks: usize,
    block_head: Vec<Id>,
    block_stop: Vec<Id>,
}

/// Triangulates a polygon given as flat coordinates, with holes starting at
/// the given vertex indices. Returns vertex-index triplets.
pub fn earcut(data: &[f64], hole_indices: &[usize], dim: usize) -> Vec<usize> {
    let mut e = Earcut {
        nodes: Vec::with_capacity(data.len() / dim.max(1) * 3 / 2 + 8),
        steiners: HashSet::new(),
        filtered_out: false,
        index_active: false,
        block_bbox: Vec::new(),
        num_blocks: 0,
        block_head: Vec::new(),
        block_stop: Vec::new(),
    };
    let has_holes = !hole_indices.is_empty();
    let outer_len = if has_holes { hole_indices[0] * dim } else { data.len() };
    let mut triangles = Vec::new();
    let Some(mut outer_node) = e.linked_list(data, 0, outer_len, dim, true) else {
        return triangles;
    };
    if e.nodes[outer_node].next == e.nodes[outer_node].prev {
        return triangles;
    }

    let (mut min_x, mut min_y, mut inv_size) = (0.0, 0.0, 0.0);
    if has_holes {
        outer_node = e.eliminate_holes(data, hole_indices, outer_node, dim);
    }
    // For larger inputs, a z-order curve hash speeds up the ear test.
    if data.len() > 80 * dim {
        min_x = data[0];
        min_y = data[1];
        let mut max_x = min_x;
        let mut max_y = min_y;
        let mut i = dim;
        while i < outer_len {
            let (x, y) = (data[i], data[i + 1]);
            if x < min_x {
                min_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if x > max_x {
                max_x = x;
            }
            if y > max_y {
                max_y = y;
            }
            i += dim;
        }
        inv_size = super::math::js_max(max_x - min_x, max_y - min_y);
        inv_size = if inv_size != 0.0 { 32767.0 / inv_size } else { 0.0 };
    }
    e.earcut_linked(outer_node, &mut triangles, min_x, min_y, inv_size);
    triangles
}

/// JavaScript truthiness of a number: not 0 and not NaN.
fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

fn signed_area(data: &[f64], start: usize, end: usize, dim: usize) -> f64 {
    let mut sum = 0.0;
    let mut j = end.wrapping_sub(dim);
    let mut i = start;
    while i < end {
        sum += (data[j] - data[i]) * (data[i + 1] + data[j + 1]);
        j = i;
        i += dim;
    }
    sum
}

#[allow(clippy::too_many_arguments)]
fn point_in_triangle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py)
        && (ax - px) * (by - py) >= (bx - px) * (ay - py)
        && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}

fn z_order(x: f64, y: f64, min_x: f64, min_y: f64, inv_size: f64) -> i32 {
    let mut x = super::math::to_int32((x - min_x) * inv_size);
    let mut y = super::math::to_int32((y - min_y) * inv_size);
    x = (x | x.wrapping_shl(8)) & 0x00FF00FF;
    x = (x | x.wrapping_shl(4)) & 0x0F0F0F0F;
    x = (x | x.wrapping_shl(2)) & 0x33333333;
    x = (x | x.wrapping_shl(1)) & 0x55555555;
    y = (y | y.wrapping_shl(8)) & 0x00FF00FF;
    y = (y | y.wrapping_shl(4)) & 0x0F0F0F0F;
    y = (y | y.wrapping_shl(2)) & 0x33333333;
    y = (y | y.wrapping_shl(1)) & 0x55555555;
    x | y.wrapping_shl(1)
}

impl Earcut {
    fn create_node(&mut self, i: usize, x: f64, y: f64) -> Id {
        let id = self.nodes.len();
        self.nodes.push(Node { i, x, y, prev: id, next: id, z: 0, prev_z: None, next_z: None });
        id
    }

    fn insert_node(&mut self, i: usize, x: f64, y: f64, last: Option<Id>) -> Id {
        let p = self.create_node(i, x, y);
        if let Some(last) = last {
            let last_next = self.nodes[last].next;
            self.nodes[p].next = last_next;
            self.nodes[p].prev = last;
            self.nodes[last_next].prev = p;
            self.nodes[last].next = p;
        }
        p
    }

    fn remove_node(&mut self, p: Id) {
        let Node { prev, next, prev_z, next_z, .. } = self.nodes[p];
        self.nodes[next].prev = prev;
        self.nodes[prev].next = next;
        if let Some(pz) = prev_z {
            self.nodes[pz].next_z = next_z;
        }
        if let Some(nz) = next_z {
            self.nodes[nz].prev_z = prev_z;
        }
        if self.index_active {
            self.grow_block(prev, next);
        }
    }

    fn x(&self, p: Id) -> f64 {
        self.nodes[p].x
    }

    fn y(&self, p: Id) -> f64 {
        self.nodes[p].y
    }

    fn next(&self, p: Id) -> Id {
        self.nodes[p].next
    }

    fn prev(&self, p: Id) -> Id {
        self.nodes[p].prev
    }

    fn equals(&self, a: Id, b: Id) -> bool {
        self.x(a) == self.x(b) && self.y(a) == self.y(b)
    }

    fn area(&self, p: Id, q: Id, r: Id) -> f64 {
        (self.y(q) - self.y(p)) * (self.x(r) - self.x(q)) - (self.x(q) - self.x(p)) * (self.y(r) - self.y(q))
    }

    fn linked_list(&mut self, data: &[f64], start: usize, end: usize, dim: usize, clockwise: bool) -> Option<Id> {
        let mut last = None;
        if clockwise == (signed_area(data, start, end, dim) > 0.0) {
            let mut i = start;
            while i < end {
                last = Some(self.insert_node(i / dim, data[i], data[i + 1], last));
                i += dim;
            }
        } else if end >= dim {
            let mut i = end - dim;
            loop {
                if i < start {
                    break;
                }
                last = Some(self.insert_node(i / dim, data[i], data[i + 1], last));
                if i < dim {
                    break;
                }
                i -= dim;
            }
        }
        if let Some(l) = last {
            if self.equals(l, self.next(l)) {
                self.remove_node(l);
                last = Some(self.next(l));
            }
        }
        last
    }

    fn filter_points(&mut self, start: Id, end: Option<Id>) -> Id {
        let mut end = end.unwrap_or(start);
        let full = end == start;
        let mut p = start;
        loop {
            let mut again = false;
            let next = self.next(p);
            if p != next
                && (self.steiners.is_empty() || !self.steiners.contains(&p))
                && (self.equals(p, next) || self.area(self.prev(p), p, next) == 0.0)
            {
                if full || p == end {
                    end = self.prev(p);
                }
                self.filtered_out = true;
                self.remove_node(p);
                p = self.prev(p);
                again = true;
            } else if full || p != end {
                p = next;
                again = !full;
            }
            if !(again || p != end) {
                break;
            }
        }
        end
    }

    fn earcut_linked(&mut self, mut ear: Id, triangles: &mut Vec<usize>, min_x: f64, min_y: f64, inv_size: f64) {
        if truthy(inv_size) {
            self.index_curve(ear, min_x, min_y, inv_size);
        }
        let mut stop = ear;
        let mut cured = false;
        while self.prev(ear) != self.next(ear) {
            let prev = self.prev(ear);
            let next = self.next(ear);
            if self.area(prev, ear, next) < 0.0
                && (if truthy(inv_size) { self.is_ear_hashed(ear, min_x, min_y, inv_size) } else { self.is_ear(ear) })
            {
                triangles.extend([self.nodes[prev].i, self.nodes[ear].i, self.nodes[next].i]);
                self.remove_node(ear);
                ear = next;
                stop = next;
                continue;
            }
            ear = next;
            if ear == stop {
                self.filtered_out = false;
                ear = self.filter_points(ear, None);
                if self.filtered_out {
                    stop = ear;
                    continue;
                }
                if !cured {
                    ear = self.cure_local_intersections(ear, triangles);
                    stop = ear;
                    cured = true;
                    continue;
                }
                self.split_earcut(ear, triangles, min_x, min_y, inv_size);
                break;
            }
        }
    }

    fn is_ear(&self, ear: Id) -> bool {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        let (ax, bx, cx, ay, by, cy) = (self.x(a), self.x(b), self.x(c), self.y(a), self.y(b), self.y(c));
        let x0 = ax.min(bx).min(cx);
        let y0 = ay.min(by).min(cy);
        let x1 = ax.max(bx).max(cx);
        let y1 = ay.max(by).max(cy);
        let mut p = self.next(c);
        while p != a {
            let (px, py) = (self.x(p), self.y(p));
            if px >= x0
                && px <= x1
                && py >= y0
                && py <= y1
                && !(ax == px && ay == py)
                && point_in_triangle(ax, ay, bx, by, cx, cy, px, py)
                && self.area(self.prev(p), p, self.next(p)) >= 0.0
            {
                return false;
            }
            p = self.next(p);
        }
        true
    }

    fn is_ear_hashed(&self, ear: Id, min_x: f64, min_y: f64, inv_size: f64) -> bool {
        let (a, b, c) = (self.prev(ear), ear, self.next(ear));
        let (ax, bx, cx, ay, by, cy) = (self.x(a), self.x(b), self.x(c), self.y(a), self.y(b), self.y(c));
        let x0 = ax.min(bx).min(cx);
        let y0 = ay.min(by).min(cy);
        let x1 = ax.max(bx).max(cx);
        let y1 = ay.max(by).max(cy);
        let min_z = z_order(x0, y0, min_x, min_y, inv_size);
        let max_z = z_order(x1, y1, min_x, min_y, inv_size);
        let inside = |p: Id| {
            let (px, py) = (self.x(p), self.y(p));
            px >= x0
                && px <= x1
                && py >= y0
                && py <= y1
                && p != c
                && !(ax == px && ay == py)
                && point_in_triangle(ax, ay, bx, by, cx, cy, px, py)
                && self.area(self.prev(p), p, self.next(p)) >= 0.0
        };
        let mut p = self.nodes[ear].prev_z;
        while let Some(pp) = p {
            if self.nodes[pp].z < min_z {
                break;
            }
            if inside(pp) {
                return false;
            }
            p = self.nodes[pp].prev_z;
        }
        let mut n = self.nodes[ear].next_z;
        while let Some(nn) = n {
            if self.nodes[nn].z > max_z {
                break;
            }
            if inside(nn) {
                return false;
            }
            n = self.nodes[nn].next_z;
        }
        true
    }

    fn cure_local_intersections(&mut self, mut start: Id, triangles: &mut Vec<usize>) -> Id {
        let mut p = start;
        let mut cured = false;
        loop {
            let a = self.prev(p);
            let b = self.next(self.next(p));
            let pn = self.next(p);
            if self.intersects(a, p, pn, b, false) && self.locally_inside(a, b) && self.locally_inside(b, a) {
                triangles.extend([self.nodes[a].i, self.nodes[p].i, self.nodes[b].i]);
                self.remove_node(p);
                let pn = self.next(p);
                self.remove_node(pn);
                p = b;
                start = b;
                cured = true;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        if cured {
            self.filter_points(p, None)
        } else {
            p
        }
    }

    fn split_earcut(&mut self, start: Id, triangles: &mut Vec<usize>, min_x: f64, min_y: f64, inv_size: f64) {
        let mut a = start;
        loop {
            let mut b = self.next(self.next(a));
            while b != self.prev(a) {
                if self.nodes[a].i != self.nodes[b].i && self.is_valid_diagonal(a, b) {
                    let mut c = self.split_polygon(a, b);
                    let an = self.next(a);
                    a = self.filter_points(a, Some(an));
                    let cn = self.next(c);
                    c = self.filter_points(c, Some(cn));
                    self.earcut_linked(a, triangles, min_x, min_y, inv_size);
                    self.earcut_linked(c, triangles, min_x, min_y, inv_size);
                    return;
                }
                b = self.next(b);
            }
            a = self.next(a);
            if a == start {
                break;
            }
        }
    }

    fn eliminate_holes(&mut self, data: &[f64], hole_indices: &[usize], mut outer_node: Id, dim: usize) -> Id {
        let mut queue = Vec::with_capacity(hole_indices.len());
        let len = hole_indices.len();
        for i in 0..len {
            let start = hole_indices[i] * dim;
            let end = if i < len - 1 { hole_indices[i + 1] * dim } else { data.len() };
            let Some(list) = self.linked_list(data, start, end, dim, false) else { continue };
            if list == self.next(list) {
                self.steiners.insert(list);
            }
            queue.push(self.get_leftmost(list));
        }
        // Array.prototype.sort is stable, and a NaN comparison counts as equal.
        queue.sort_by(|&a, &b| {
            let v = self.compare_xy_slope(a, b);
            if v < 0.0 {
                std::cmp::Ordering::Less
            } else if v > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        self.build_block_index(data.len() as f64 / dim as f64, hole_indices.len());
        self.index_segment(outer_node, outer_node);
        self.index_active = true;
        for &hole in &queue {
            outer_node = self.eliminate_hole(hole, outer_node);
        }
        self.index_active = false;
        self.filter_points(outer_node, None)
    }

    fn compare_xy_slope(&self, a: Id, b: Id) -> f64 {
        let dx = self.x(a) - self.x(b);
        if truthy(dx) {
            return dx;
        }
        let dy = self.y(a) - self.y(b);
        if truthy(dy) {
            return dy;
        }
        let (an, bn) = (self.next(a), self.next(b));
        (self.y(an) - self.y(a)) / (self.x(an) - self.x(a)) - (self.y(bn) - self.y(b)) / (self.x(bn) - self.x(b))
    }

    fn eliminate_hole(&mut self, hole: Id, outer_node: Id) -> Id {
        let Some(bridge) = self.find_hole_bridge(hole, outer_node) else {
            return outer_node;
        };
        let bridge_reverse = self.split_polygon(bridge, hole);
        let bridge2 = self.next(bridge_reverse);
        let stop = self.next(bridge2);
        self.index_segment(bridge, stop);
        let brn = self.next(bridge_reverse);
        self.filter_points(bridge_reverse, Some(brn));
        let bn = self.next(bridge);
        self.filter_points(bridge, Some(bn))
    }

    fn build_block_index(&mut self, max_nodes: f64, num_holes: usize) {
        let max_blocks = ((max_nodes + 2.0 * num_holes as f64) / K as f64).ceil() as usize + num_holes + 2;
        if self.block_bbox.len() < max_blocks * 4 {
            self.block_bbox = vec![0.0; max_blocks * 4];
        }
        self.num_blocks = 0;
    }

    fn index_segment(&mut self, head: Id, stop: Id) {
        let mut p = head;
        loop {
            let b = self.num_blocks;
            self.num_blocks += 1;
            if self.block_head.len() <= b {
                self.block_head.resize(b + 1, 0);
                self.block_stop.resize(b + 1, 0);
            }
            if self.block_bbox.len() < (b + 1) * 4 {
                self.block_bbox.resize((b + 1) * 4, 0.0);
            }
            self.block_head[b] = p;
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (self.x(p), self.y(p), self.x(p), self.y(p));
            let mut k = 0;
            loop {
                let c = self.next(p);
                self.nodes[p].z = b as i32;
                let (cx, cy) = (self.x(c), self.y(c));
                if cx < min_x {
                    min_x = cx;
                }
                if cx > max_x {
                    max_x = cx;
                }
                if cy < min_y {
                    min_y = cy;
                }
                if cy > max_y {
                    max_y = cy;
                }
                p = c;
                k += 1;
                if !(k < K && p != stop) {
                    break;
                }
            }
            self.block_stop[b] = p;
            let g = b * 4;
            self.block_bbox[g..g + 4].copy_from_slice(&[min_x, min_y, max_x, max_y]);
            if p == stop {
                break;
            }
        }
    }

    fn grow_block(&mut self, head: Id, tail: Id) {
        let g = self.nodes[head].z as usize * 4;
        if g + 3 >= self.block_bbox.len() {
            return;
        }
        let (tx, ty) = (self.x(tail), self.y(tail));
        if tx < self.block_bbox[g] {
            self.block_bbox[g] = tx;
        }
        if ty < self.block_bbox[g + 1] {
            self.block_bbox[g + 1] = ty;
        }
        if tx > self.block_bbox[g + 2] {
            self.block_bbox[g + 2] = tx;
        }
        if ty > self.block_bbox[g + 3] {
            self.block_bbox[g + 3] = ty;
        }
    }

    fn is_live(&self, p: Id) -> bool {
        self.next(self.prev(p)) == p
    }

    fn live_block_stop(&mut self, b: usize) -> Id {
        let mut stop = self.block_stop[b];
        while !self.is_live(stop) {
            stop = self.next(stop);
        }
        self.block_stop[b] = stop;
        stop
    }

    fn live_block_head(&mut self, b: usize) -> Id {
        let mut head = self.block_head[b];
        while !self.is_live(head) {
            head = self.next(head);
        }
        self.block_head[b] = head;
        head
    }

    fn find_hole_bridge(&mut self, hole: Id, outer_node: Id) -> Option<Id> {
        let (hx, hy) = (self.x(hole), self.y(hole));
        let mut qx = f64::NEG_INFINITY;
        let mut m: Option<Id> = None;
        if self.equals(hole, outer_node) {
            return Some(outer_node);
        }
        // Find a segment intersected by a ray from the hole's leftmost point
        // to the left; its endpoint with the smaller x is a bridge candidate.
        for b in 0..self.num_blocks {
            let g = b * 4;
            let bb = &self.block_bbox[g..g + 4];
            if hy < bb[1] || hy > bb[3] || bb[0] > hx || bb[2] <= qx {
                continue;
            }
            let stop = self.live_block_stop(b);
            let mut p = self.live_block_head(b);
            loop {
                if self.is_live(p) {
                    let pn = self.next(p);
                    if self.equals(hole, pn) {
                        return Some(pn);
                    } else if hy <= self.y(p) && hy >= self.y(pn) && self.y(pn) != self.y(p) {
                        let x = self.x(p) + (hy - self.y(p)) * (self.x(pn) - self.x(p)) / (self.y(pn) - self.y(p));
                        if x <= hx && x > qx {
                            qx = x;
                            m = Some(if self.x(p) < self.x(pn) { p } else { pn });
                            if x == hx {
                                return m;
                            }
                        }
                    }
                }
                p = self.next(p);
                if p == stop {
                    break;
                }
            }
        }
        let mut m = m?;

        // Of the points inside the triangle (hole point, intersection, m),
        // pick the one with the smallest angle to the ray.
        let (mx, my) = (self.x(m), self.y(m));
        let tmin_y = hy.min(my);
        let tmax_y = hy.max(my);
        let mut tan_min = f64::INFINITY;
        for b in 0..self.num_blocks {
            let g = b * 4;
            let bb = &self.block_bbox[g..g + 4];
            if bb[2] < mx || bb[0] > hx || bb[3] < tmin_y || bb[1] > tmax_y {
                continue;
            }
            let stop = self.live_block_stop(b);
            let mut p = self.live_block_head(b);
            loop {
                let (px, py) = (self.x(p), self.y(p));
                if self.is_live(p)
                    && hx >= px
                    && px >= mx
                    && hx != px
                    && point_in_triangle(
                        if hy < my { hx } else { qx },
                        hy,
                        mx,
                        my,
                        if hy < my { qx } else { hx },
                        hy,
                        px,
                        py,
                    )
                {
                    let tan = (hy - py).abs() / (hx - px);
                    let pn = self.next(p);
                    if (self.locally_inside(p, hole) || (py == hy && self.y(pn) == hy && self.x(pn) > hx))
                        && (tan < tan_min
                            || (tan == tan_min && (px > self.x(m) || (px == self.x(m) && self.sector_contains_sector(m, p)))))
                    {
                        m = p;
                        tan_min = tan;
                    }
                }
                p = self.next(p);
                if p == stop {
                    break;
                }
            }
        }
        Some(m)
    }

    fn sector_contains_sector(&self, m: Id, p: Id) -> bool {
        self.area(self.prev(m), m, self.prev(p)) < 0.0 && self.area(self.next(p), m, self.next(m)) < 0.0
    }

    fn index_curve(&mut self, start: Id, min_x: f64, min_y: f64, inv_size: f64) {
        let mut order = Vec::new();
        let mut p = start;
        loop {
            let z = z_order(self.x(p), self.y(p), min_x, min_y, inv_size);
            self.nodes[p].z = z;
            order.push(p);
            p = self.next(p);
            if p == start {
                break;
            }
        }
        if order.len() <= 32 {
            // Insertion sort on the (signed) z values.
            for i in 1..order.len() {
                let node = order[i];
                let z = self.nodes[node].z;
                let mut j = i;
                while j > 0 && self.nodes[order[j - 1]].z > z {
                    order[j] = order[j - 1];
                    j -= 1;
                }
                order[j] = node;
            }
        } else {
            // Four stable 8-bit radix passes on z as an unsigned 32-bit value.
            for shift in [0u32, 8, 16, 24] {
                let mut counts = [0usize; 256];
                for &n in &order {
                    counts[((self.nodes[n].z as u32 >> shift) & 0xff) as usize] += 1;
                }
                let mut sum = 0;
                for c in counts.iter_mut() {
                    let v = *c;
                    *c = sum;
                    sum += v;
                }
                let mut sorted = vec![0; order.len()];
                for &n in &order {
                    let bucket = ((self.nodes[n].z as u32 >> shift) & 0xff) as usize;
                    sorted[counts[bucket]] = n;
                    counts[bucket] += 1;
                }
                order = sorted;
            }
        }
        let mut prev: Option<Id> = None;
        for &node in &order {
            self.nodes[node].prev_z = prev;
            if let Some(pr) = prev {
                self.nodes[pr].next_z = Some(node);
            }
            prev = Some(node);
        }
        if let Some(pr) = prev {
            self.nodes[pr].next_z = None;
        }
    }

    fn get_leftmost(&self, start: Id) -> Id {
        let mut p = start;
        let mut leftmost = start;
        loop {
            if self.x(p) < self.x(leftmost) || (self.x(p) == self.x(leftmost) && self.y(p) < self.y(leftmost)) {
                leftmost = p;
            }
            p = self.next(p);
            if p == start {
                break;
            }
        }
        leftmost
    }

    fn is_valid_diagonal(&self, a: Id, b: Id) -> bool {
        let zero_length = self.equals(a, b)
            && self.area(self.prev(a), a, self.next(a)) > 0.0
            && self.area(self.prev(b), b, self.next(b)) > 0.0;
        self.nodes[self.next(a)].i != self.nodes[b].i
            && (zero_length
                || self.locally_inside(a, b)
                    && self.locally_inside(b, a)
                    && (self.area(self.prev(a), a, self.prev(b)) != 0.0 || self.area(a, self.prev(b), b) != 0.0))
            && !self.intersects_polygon(a, b)
            && (zero_length || self.middle_inside(a, b))
    }

    fn intersects(&self, p1: Id, q1: Id, p2: Id, q2: Id, include_boundary: bool) -> bool {
        let o1 = self.area(p1, q1, p2);
        let o2 = self.area(p1, q1, q2);
        let o3 = self.area(p2, q2, p1);
        let o4 = self.area(p2, q2, q1);
        if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0)) && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0)) {
            return true;
        }
        if !include_boundary {
            return false;
        }
        (o1 == 0.0 && self.on_segment(p1, p2, q1))
            || (o2 == 0.0 && self.on_segment(p1, q2, q1))
            || (o3 == 0.0 && self.on_segment(p2, p1, q2))
            || (o4 == 0.0 && self.on_segment(p2, q1, q2))
    }

    fn on_segment(&self, p: Id, q: Id, r: Id) -> bool {
        self.x(q) <= self.x(p).max(self.x(r))
            && self.x(q) >= self.x(p).min(self.x(r))
            && self.y(q) <= self.y(p).max(self.y(r))
            && self.y(q) >= self.y(p).min(self.y(r))
    }

    fn intersects_polygon(&self, a: Id, b: Id) -> bool {
        let min_x = self.x(a).min(self.x(b));
        let max_x = self.x(a).max(self.x(b));
        let min_y = self.y(a).min(self.y(b));
        let max_y = self.y(a).max(self.y(b));
        let (ai, bi) = (self.nodes[a].i, self.nodes[b].i);
        let mut p = a;
        loop {
            let n = self.next(p);
            let (px, py, nx, ny) = (self.x(p), self.y(p), self.x(n), self.y(n));
            let outside = (px > max_x && nx > max_x)
                || (px < min_x && nx < min_x)
                || (py > max_y && ny > max_y)
                || (py < min_y && ny < min_y);
            if !outside {
                let (pi, ni) = (self.nodes[p].i, self.nodes[n].i);
                if pi != ai && ni != ai && pi != bi && ni != bi && self.intersects(p, n, a, b, true) {
                    return true;
                }
            }
            p = n;
            if p == a {
                break;
            }
        }
        false
    }

    fn locally_inside(&self, a: Id, b: Id) -> bool {
        if self.area(self.prev(a), a, self.next(a)) < 0.0 {
            self.area(a, b, self.next(a)) >= 0.0 && self.area(a, self.prev(a), b) >= 0.0
        } else {
            self.area(a, b, self.prev(a)) < 0.0 || self.area(a, self.next(a), b) < 0.0
        }
    }

    fn middle_inside(&self, a: Id, b: Id) -> bool {
        let mut p = a;
        let mut inside = false;
        let px = (self.x(a) + self.x(b)) / 2.0;
        let py = (self.y(a) + self.y(b)) / 2.0;
        loop {
            let n = self.next(p);
            let (x0, y0, x1, y1) = (self.x(p), self.y(p), self.x(n), self.y(n));
            if ((y0 > py) != (y1 > py)) && (px < (x1 - x0) * (py - y0) / (y1 - y0) + x0) {
                inside = !inside;
            }
            p = n;
            if p == a {
                break;
            }
        }
        inside
    }

    fn split_polygon(&mut self, a: Id, b: Id) -> Id {
        let (ai, ax, ay) = (self.nodes[a].i, self.x(a), self.y(a));
        let (bi, bx, by) = (self.nodes[b].i, self.x(b), self.y(b));
        let a2 = self.create_node(ai, ax, ay);
        let b2 = self.create_node(bi, bx, by);
        let an = self.next(a);
        let bp = self.prev(b);
        self.nodes[a].next = b;
        self.nodes[b].prev = a;
        self.nodes[a2].next = an;
        self.nodes[an].prev = a2;
        self.nodes[b2].next = a2;
        self.nodes[a2].prev = b2;
        self.nodes[bp].next = b2;
        self.nodes[b2].prev = bp;
        b2
    }
}
