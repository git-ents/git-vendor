/// Errors produced by [`crate::DepotRepository`] operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error")]
    Io(#[from] std::io::Error),
    #[error("no working directory")]
    NoWorkdir,
    #[error("invalid vendor config: {0}")]
    Config(String),
    #[error("invalid vendor name: {0}")]
    InvalidName(String),
    #[error("invalid vendor url: {0}")]
    InvalidUrl(String),
    #[error("fetch failed: {0}")]
    Fetch(String),
    #[error(transparent)]
    Gix(Box<dyn std::error::Error + Send + Sync + 'static>),
}

macro_rules! impl_gix_from {
    ($($ty:path),* $(,)?) => {
        $(
            impl From<$ty> for Error {
                fn from(e: $ty) -> Self {
                    Error::Gix(Box::new(e))
                }
            }
        )*
    };
}

impl_gix_from! {
    gix::object::commit::Error,
    gix::object::find::existing::Error,
    gix::object::find::existing::with_conversion::Error,
    gix::reference::edit::Error,
    gix::reference::find::Error,
    gix::reference::find::existing::Error,
    gix::reference::peel::Error,
    gix::remote::connect::Error,
    gix::remote::fetch::Error,
    gix::remote::fetch::prepare::Error,
    gix::remote::init::Error,
    gix::refspec::parse::Error,
    gix::object::tree::editor::init::Error,
    gix::object::tree::editor::write::Error,
    gix::objs::tree::editor::Error,
    gix::traverse::tree::breadthfirst::Error,
    gix::repository::index_from_tree::Error,
    gix::repository::merge_trees::Error,
    gix::repository::tree_merge_options::Error,
    gix::config::attribute_stack::Error,
}
