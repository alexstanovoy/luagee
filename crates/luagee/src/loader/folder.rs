//! The folder loader (feature `fs`): the only part of the library that touches the host's files.

use crate::{Loader, ModuleName};

/// Modules as `<root>/<name>.lua` files, never outside the root: a symlink anywhere below the root, a file that is
/// not a regular one, or one larger than the bound, is not found. (Feature `fs`.)
#[derive(Clone, Debug)]
pub struct FolderLoader {
    root: std::path::PathBuf,
    max_bytes: u64,
}

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

impl Loader for FolderLoader {
    fn load(&mut self, module: &ModuleName) -> Option<Vec<u8>> {
        self.read(module).ok().flatten()
    }
}
