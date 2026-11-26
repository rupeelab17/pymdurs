//! Cadastre (parcels) — thin alias over [`IgnLayer`].

use anyhow::Result;
use std::ops::{Deref, DerefMut};

use super::ign_layer::{IgnLayer, IgnLayerKind};

/// Cadastre collector (IGN WFS). Public API preserved for existing callers.
pub struct Cadastre(IgnLayer);

impl Cadastre {
    pub fn new(output_path: Option<String>) -> Result<Self> {
        Ok(Self(IgnLayer::new(IgnLayerKind::Cadastre, output_path)?))
    }

    /// Consuming run — cannot go through `Deref` (would move out of the wrapper).
    pub fn run(self) -> Result<Self> {
        Ok(Self(self.0.run()?))
    }
}

impl Deref for Cadastre {
    type Target = IgnLayer;
    fn deref(&self) -> &IgnLayer {
        &self.0
    }
}

impl DerefMut for Cadastre {
    fn deref_mut(&mut self) -> &mut IgnLayer {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadastre_new() {
        let c = Cadastre::new(None).unwrap();
        assert_eq!(c.kind(), IgnLayerKind::Cadastre);
        assert!(!c.get_output_path().as_os_str().is_empty());
        assert_eq!(c.epsg(), 2154);
    }

    #[test]
    fn cadastre_set_bbox_crs() {
        let mut c = Cadastre::new(None).unwrap();
        c.set_bbox(-1.15, 46.18, -1.14, 46.19);
        c.set_crs(2154);
        assert!(c.bbox().is_some());
        assert_eq!(c.epsg(), 2154);
    }
}
