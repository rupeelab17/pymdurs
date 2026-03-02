pub mod collect;
pub mod commons;
pub mod geo_core;
pub mod geometric;
pub mod thermal;

// umep-rust integration
// Urban Meteorology and Environmental Processing functionality
// Available when "umep" feature is enabled (included in default features)
// Note: umep_rust is not a separate crate - it's part of umep-rust repository
// If you need UMEP functionality, use the solweig Python package instead
// #[cfg(feature = "umep")]
// pub use umep_rust;
