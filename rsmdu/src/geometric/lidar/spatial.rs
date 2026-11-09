//! Spatial indexes for LiDAR point queries.

use std::collections::HashMap;

use super::types::LidarPoint;

/// Cell key for spatial grid indexing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GridCellKey {
    pub(super) col: i64,
    pub(super) row: i64,
}

/// Spatial grid index for fast point queries
/// Uses a regular grid to bucket points by location
#[derive(Debug)]
pub(super) struct SpatialGridIndex {
    cell_size: f64,
    origin_x: f64,
    origin_y: f64,
    cells: HashMap<GridCellKey, Vec<usize>>,
    point_count: usize,
}

impl SpatialGridIndex {
    pub(super) fn new(cell_size: f64, bounds: Option<(f64, f64, f64, f64)>) -> Self {
        let (origin_x, origin_y) = bounds
            .map(|(min_x, min_y, _, _)| (min_x, min_y))
            .unwrap_or((0.0, 0.0));
        SpatialGridIndex {
            cell_size,
            origin_x,
            origin_y,
            cells: HashMap::new(),
            point_count: 0,
        }
    }

    #[inline]
    pub(super) fn cell_key(&self, x: f64, y: f64) -> GridCellKey {
        GridCellKey {
            col: ((x - self.origin_x) / self.cell_size).floor() as i64,
            row: ((y - self.origin_y) / self.cell_size).floor() as i64,
        }
    }

    pub(super) fn build_from_points(points: &[LidarPoint], cell_size: f64) -> Self {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        let mut index = SpatialGridIndex::new(cell_size, Some((min_x, min_y, max_x, max_y)));
        for (i, p) in points.iter().enumerate() {
            let key = index.cell_key(p.x, p.y);
            index.cells.entry(key).or_insert_with(Vec::new).push(i);
        }
        index.point_count = points.len();
        index
    }

    pub(super) fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        let min_col = ((min_x - self.origin_x) / self.cell_size).floor() as i64;
        let max_col = ((max_x - self.origin_x) / self.cell_size).floor() as i64;
        let min_row = ((min_y - self.origin_y) / self.cell_size).floor() as i64;
        let max_row = ((max_y - self.origin_y) / self.cell_size).floor() as i64;
        let mut result = Vec::new();
        for col in min_col..=max_col {
            for row in min_row..=max_row {
                let key = GridCellKey { col, row };
                if let Some(indices) = self.cells.get(&key) {
                    result.extend(indices);
                }
            }
        }
        result
    }

    pub(super) fn stats(&self) -> SpatialIndexStats {
        let cell_count = self.cells.len();
        let total_points = self.point_count;
        let avg_points_per_cell = if cell_count > 0 {
            total_points as f64 / cell_count as f64
        } else {
            0.0
        };
        let max_points_in_cell = self.cells.values().map(|v| v.len()).max().unwrap_or(0);
        SpatialIndexStats {
            cell_count,
            total_points,
            avg_points_per_cell,
            max_points_in_cell,
            cell_size: self.cell_size,
        }
    }
}

#[derive(Debug)]
pub(super) struct SpatialIndexStats {
    cell_count: usize,
    total_points: usize,
    avg_points_per_cell: f64,
    max_points_in_cell: usize,
    cell_size: f64,
}

impl std::fmt::Display for SpatialIndexStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SpatialIndex: {} cells, {} points, {:.1} avg/cell, {} max/cell, {:.1}m cell size",
            self.cell_count,
            self.total_points,
            self.avg_points_per_cell,
            self.max_points_in_cell,
            self.cell_size
        )
    }
}

#[derive(Debug)]
pub(super) struct OctreeNode {
    bounds: (f64, f64, f64, f64, f64, f64),
    points: Vec<usize>,
    children: Option<Box<[Option<OctreeNode>; 4]>>,
    depth: u8,
}

impl OctreeNode {
    const MAX_POINTS_PER_NODE: usize = 1000;
    const MAX_DEPTH: u8 = 12;

    pub(super) fn new_leaf(bounds: (f64, f64, f64, f64, f64, f64), depth: u8) -> Self {
        OctreeNode {
            bounds,
            points: Vec::new(),
            children: None,
            depth,
        }
    }

    #[inline]
    fn contains_xy(&self, x: f64, y: f64) -> bool {
        x >= self.bounds.0 && x <= self.bounds.3 && y >= self.bounds.1 && y <= self.bounds.4
    }

    #[inline]
    fn intersects_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        !(self.bounds.3 < min_x
            || self.bounds.0 > max_x
            || self.bounds.4 < min_y
            || self.bounds.1 > max_y)
    }

    pub(super) fn quadrant_for_point(&self, x: f64, y: f64) -> usize {
        let mid_x = (self.bounds.0 + self.bounds.3) / 2.0;
        let mid_y = (self.bounds.1 + self.bounds.4) / 2.0;
        match (x >= mid_x, y >= mid_y) {
            (false, false) => 0,
            (true, false) => 1,
            (false, true) => 2,
            (true, true) => 3,
        }
    }

    pub(super) fn child_bounds(&self, quadrant: usize) -> (f64, f64, f64, f64, f64, f64) {
        let mid_x = (self.bounds.0 + self.bounds.3) / 2.0;
        let mid_y = (self.bounds.1 + self.bounds.4) / 2.0;
        match quadrant {
            0 => (
                self.bounds.0,
                self.bounds.1,
                self.bounds.2,
                mid_x,
                mid_y,
                self.bounds.5,
            ),
            1 => (
                mid_x,
                self.bounds.1,
                self.bounds.2,
                self.bounds.3,
                mid_y,
                self.bounds.5,
            ),
            2 => (
                self.bounds.0,
                mid_y,
                self.bounds.2,
                mid_x,
                self.bounds.4,
                self.bounds.5,
            ),
            3 => (
                mid_x,
                mid_y,
                self.bounds.2,
                self.bounds.3,
                self.bounds.4,
                self.bounds.5,
            ),
            _ => unreachable!(),
        }
    }

    fn insert(&mut self, point_idx: usize, points: &[LidarPoint]) {
        let point = &points[point_idx];
        if !self.contains_xy(point.x, point.y) {
            return;
        }
        if self.children.is_some() {
            let quadrant = self.quadrant_for_point(point.x, point.y);
            let child_bounds = self.child_bounds(quadrant);
            let depth = self.depth;
            let children = self.children.as_mut().unwrap();
            if children[quadrant].is_none() {
                children[quadrant] = Some(OctreeNode::new_leaf(child_bounds, depth + 1));
            }
            if let Some(ref mut child) = children[quadrant] {
                child.insert(point_idx, points);
            }
            return;
        }
        self.points.push(point_idx);
        if self.points.len() > Self::MAX_POINTS_PER_NODE && self.depth < Self::MAX_DEPTH {
            self.split(points);
        }
    }

    fn split(&mut self, points: &[LidarPoint]) {
        self.children = Some(Box::new([None, None, None, None]));
        let old_points = std::mem::take(&mut self.points);
        for point_idx in old_points {
            self.insert(point_idx, points);
        }
    }

    fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64, result: &mut Vec<usize>) {
        if !self.intersects_bbox(min_x, min_y, max_x, max_y) {
            return;
        }
        result.extend(&self.points);
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                child.query_bbox(min_x, min_y, max_x, max_y, result);
            }
        }
    }

    fn count_points(&self) -> usize {
        let mut count = self.points.len();
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                count += child.count_points();
            }
        }
        count
    }

    fn count_nodes(&self) -> usize {
        let mut count = 1;
        if let Some(ref children) = self.children {
            for child in children.iter().flatten() {
                count += child.count_nodes();
            }
        }
        count
    }
}

#[derive(Debug)]
pub struct QuadtreeSpatialIndex {
    root: OctreeNode,
}

impl QuadtreeSpatialIndex {
    pub(crate) fn build(points: &[LidarPoint]) -> Self {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;
        for p in points {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            min_z = min_z.min(p.z);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
            max_z = max_z.max(p.z);
        }
        let padding = 0.001;
        min_x -= padding;
        min_y -= padding;
        max_x += padding;
        max_y += padding;
        let mut root = OctreeNode::new_leaf((min_x, min_y, min_z, max_x, max_y, max_z), 0);
        for i in 0..points.len() {
            root.insert(i, points);
        }
        QuadtreeSpatialIndex { root }
    }

    pub fn query_bbox(&self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Vec<usize> {
        let mut result = Vec::new();
        self.root
            .query_bbox(min_x, min_y, max_x, max_y, &mut result);
        result
    }

    pub fn stats(&self) -> String {
        format!(
            "Quadtree: {} nodes, {} points indexed",
            self.root.count_nodes(),
            self.root.count_points()
        )
    }
}
