//! The folder loader keeps `require` inside its root, whatever is on disk below it.
#![cfg(feature = "fs")]

use std::path::{Path, PathBuf};

use luagee::{FolderLoader, Loader, ModuleName};

/// A fresh folder for one test, named by the process and the test.
fn folder(test: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("luagee-loader-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(path.join("root/lib")).expect("a test folder");
    std::fs::write(path.join("root/lib/ok.lua"), b"return 1").expect("a module");
    std::fs::write(path.join("secret.lua"), b"return 'outside'").expect("a file outside the root");
    path
}

fn load(root: &Path, name: &str, max: u64) -> Option<Vec<u8>> {
    let mut loader = FolderLoader::new(root, max).expect("the root exists");
    loader.load(&name.parse::<ModuleName>().expect("a module name"))
}

#[test]
fn a_module_inside_the_root_loads() {
    let base = folder("inside");
    assert_eq!(
        load(&base.join("root"), "lib/ok", 1024).as_deref(),
        Some(&b"return 1"[..])
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_below_the_root_is_not_followed() {
    let base = folder("symlink");
    std::os::unix::fs::symlink(base.join("secret.lua"), base.join("root/lib/link.lua"))
        .expect("a file link");
    std::os::unix::fs::symlink(&base, base.join("root/up")).expect("a folder link");
    assert_eq!(load(&base.join("root"), "lib/link", 1024), None);
    assert_eq!(load(&base.join("root"), "up/secret", 1024), None);
}

#[test]
fn a_folder_or_a_missing_file_is_not_found() {
    let base = folder("kinds");
    std::fs::create_dir_all(base.join("root/dir.lua")).expect("a folder named like a module");
    assert_eq!(load(&base.join("root"), "dir", 1024), None);
    assert_eq!(load(&base.join("root"), "missing", 1024), None);
}

#[test]
fn a_file_past_the_bound_is_not_found() {
    let base = folder("size");
    std::fs::write(base.join("root/big.lua"), vec![b' '; 2048]).expect("a big module");
    assert_eq!(load(&base.join("root"), "big", 1024), None);
    assert!(load(&base.join("root"), "big", 4096).is_some());
}
