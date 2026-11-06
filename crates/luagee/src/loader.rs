//! Where `require` and `dofile` get source: a loader the host gives, and a folder loader confined to its root.

use crate::ModuleName;

/// The host's source of modules for `require` and `dofile`. The interpreter hands it a [`ModuleName`] (checked to
/// name nothing outside a folder) and runs the bytes it returns; `None` is "module not found". A closure taking a
/// `&ModuleName` is a loader.
pub trait Loader {
    /// The source of a module, or `None` when there is none.
    fn load(&mut self, module: &ModuleName) -> Option<Vec<u8>>;
}

impl<F: FnMut(&ModuleName) -> Option<Vec<u8>>> Loader for F {
    fn load(&mut self, module: &ModuleName) -> Option<Vec<u8>> {
        self(module)
    }
}

#[cfg(feature = "fs")]
mod folder;
#[cfg(feature = "fs")]
pub use folder::FolderLoader;
