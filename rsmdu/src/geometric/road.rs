//! Road (routes) — thin alias over [`IgnLayer`].

use anyhow::Result;
use std::ops::{Deref, DerefMut};

use super::ign_layer::{IgnLayer, IgnLayerKind};

/// Road collector (IGN WFS). Public API preserved for existing callers.
pub struct Road(IgnLayer);

impl Road {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        Ok(Self(IgnLayer::new(IgnLayerKind::Road, output_path)?))
    }

    /// Consuming run — cannot go through `Deref` (would move out of the wrapper).
    pub fn run(self) -> Result<Self> {
        Ok(Self(self.0.run()?))
    }
}

impl Deref for Road {
    type Target = IgnLayer;
    fn deref(&self) -> &IgnLayer {
        &self.0
    }
}

impl DerefMut for Road {
    fn deref_mut(&mut self) -> &mut IgnLayer {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn road_new() {
        let r = Road::new(None).unwrap();
        assert_eq!(r.kind(), IgnLayerKind::Road);
        assert!(!r.get_output_path().as_os_str().is_empty());
        assert_eq!(r.epsg(), 2154);
    }

    #[test]
    fn road_set_bbox_crs() {
        let mut r = Road::new(None).unwrap();
        r.set_bbox(-1.15, 46.18, -1.14, 46.19);
        r.set_crs(2154);
        assert!(r.bbox().is_some());
        assert_eq!(r.epsg(), 2154);
    }
}
