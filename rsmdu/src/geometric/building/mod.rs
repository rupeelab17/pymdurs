//! Buildings — load, height processing, export.

mod export;
mod load;
mod process;
mod types;

pub use types::{Building, BuildingCollection};

#[cfg(test)]
mod tests;
