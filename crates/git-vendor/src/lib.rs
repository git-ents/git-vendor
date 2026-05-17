//! Manage anonymous remotes to vendor external repository content.

mod error;
mod vendor;

pub use error::Error;
pub use vendor::{DepotRepository, VendorEntry};
