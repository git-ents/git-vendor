//! Manage anonymous remotes to vendor external repository content.

mod error;
mod vendor;

pub use error::Error;
pub use vendor::{VendorEntry, VendorRepository};

impl VendorRepository for gix::Repository {
    fn list_vendors(&self) -> Result<Vec<VendorEntry>, Error> {
        todo!()
    }

    fn get_vendor(&self, _name: &str) -> Result<Option<VendorEntry>, Error> {
        todo!()
    }

    fn add_vendor(&self, _entry: &VendorEntry) -> Result<gix::index::State, Error> {
        todo!()
    }

    fn remove_vendor(&self, _name: &str) -> Result<(), Error> {
        todo!()
    }

    fn fetch_vendor(&self, _entry: &VendorEntry) -> Result<gix::ObjectId, Error> {
        todo!()
    }

    fn track_vendor_pattern(&self, _entry: &VendorEntry) -> Result<(), Error> {
        todo!()
    }

    fn refresh_vendor_attrs(&self, _entry: &VendorEntry) -> Result<(), Error> {
        todo!()
    }

    fn merge_vendor(&self, _entry: &VendorEntry) -> Result<gix::index::State, Error> {
        todo!()
    }

    fn vendor_attributes(&self, _entry: &VendorEntry) -> Result<Vec<String>, Error> {
        todo!()
    }
}
