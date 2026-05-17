//! Manage anonymous remotes to vendor external repository content.

mod error;
mod vendor;

pub mod exe;

pub use error::Error;
pub use exe::VendorWorktree;
pub use vendor::{
    PatternMapping, VendorConfig, VendorEntry, VendorMerge, VendorName, VendorRepository,
    VendorStatus,
};

impl VendorRepository for gix::Repository {
    fn fetch_vendor(&self, _entry: &VendorEntry) -> Result<gix::ObjectId, Error> {
        todo!()
    }

    fn vendor_tip(&self, _entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error> {
        todo!()
    }

    fn vendor_status(&self, _entry: &VendorEntry) -> Result<VendorStatus, Error> {
        todo!()
    }

    fn upstream_tree(
        &self,
        _entry: &VendorEntry,
        _commit: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error> {
        todo!()
    }

    fn base_tree(&self, _entry: &VendorEntry) -> Result<Option<gix::ObjectId>, Error> {
        todo!()
    }

    fn vendor_tree(
        &self,
        _entry: &VendorEntry,
        _ours: gix::ObjectId,
    ) -> Result<gix::ObjectId, Error> {
        todo!()
    }

    fn vendor_paths(&self, _entry: &VendorEntry) -> Result<Vec<String>, Error> {
        todo!()
    }

    fn merge_vendor(
        &self,
        _entry: &VendorEntry,
        _ours: gix::ObjectId,
        _theirs: gix::ObjectId,
    ) -> Result<VendorMerge, Error> {
        todo!()
    }

    fn commit_vendor(
        &self,
        _entry: &VendorEntry,
        _parent: gix::ObjectId,
        _merge: &VendorMerge,
    ) -> Result<gix::ObjectId, Error> {
        todo!()
    }
}
