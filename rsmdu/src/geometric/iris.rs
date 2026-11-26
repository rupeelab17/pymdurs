//! IRIS statistical units — thin alias over [`IgnLayer`].

use anyhow::Result;
use std::ops::{Deref, DerefMut};

use super::ign_layer::{IgnLayer, IgnLayerKind};

/// Iris collector (IGN WFS). Public API preserved for existing callers.
pub struct Iris(IgnLayer);

impl Iris {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        Ok(Self(IgnLayer::new(IgnLayerKind::Iris, output_path)?))
    }
}

impl Deref for Iris {
    type Target = IgnLayer;
    fn deref(&self) -> &IgnLayer {
        &self.0
    }
}

impl DerefMut for Iris {
    fn deref_mut(&mut self) -> &mut IgnLayer {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iris_new() {
        let i = Iris::new(None).unwrap();
        assert_eq!(i.kind(), IgnLayerKind::Iris);
        assert!(!i.get_output_path().as_os_str().is_empty());
        assert_eq!((&*i).geo_core.get_epsg(), 2154);
    }

    #[test]
    fn iris_set_bbox_crs() {
        let mut i = Iris::new(None).unwrap();
        i.set_bbox(-1.15, 46.18, -1.14, 46.19);
        i.set_crs(2154);
        assert!(i.bbox().is_some());
        assert_eq!((&*i).geo_core.get_epsg(), 2154);
    }
}
