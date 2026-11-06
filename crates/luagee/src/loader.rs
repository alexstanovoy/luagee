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

/// Modules as `<root>/<name>.lua` files, never outside the root: a symlink anywhere below the root, a file that is
/// not a regular one, or one larger than the bound, is not found. (Feature `fs`.)
#[cfg(feature = "fs")]
#[derive(Clone, Debug)]
pub struct FolderLoader {
    root: std::path::PathBuf,
    max_bytes: u64,
}

#[cfg(feature = "fs")]
impl FolderLoader {
    /// A loader reading below `root` (which must exist), files of at most `max_bytes`.
    pub fn new(root: &std::path::Path, max_bytes: u64) -> std::io::Result<FolderLoader> {
        Ok(FolderLoader {
            root: root.canonicalize()?,
            max_bytes,
        })
    }

    fn read(&self, module: &ModuleName) -> std::io::Result<Option<Vec<u8>>> {
        use std::io::Read as _;
        let mut path = self.root.clone();
        let file = module.file();
        let parts: Vec<&str> = file.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            path.push(part);
            let kind = std::fs::symlink_metadata(&path)?.file_type();
            let last = i + 1 == parts.len();
            // Every component must be what it says, never a link that could lead out of the root.
            if kind.is_symlink() || (last && !kind.is_file()) || (!last && !kind.is_dir()) {
                return Ok(None);
            }
        }
        let opened = std::fs::File::open(&path)?;
        if !opened.metadata()?.is_file() || !path.canonicalize()?.starts_with(&self.root) {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        opened.take(self.max_bytes + 1).read_to_end(&mut bytes)?;
        Ok((bytes.len() as u64 <= self.max_bytes).then_some(bytes))
    }
}

#[cfg(feature = "fs")]
impl Loader for FolderLoader {
    fn load(&mut self, module: &ModuleName) -> Option<Vec<u8>> {
        self.read(module).ok().flatten()
    }
}
