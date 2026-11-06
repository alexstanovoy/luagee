use super::{ModuleName, ModuleNameError};

fn file(name: &str) -> Result<String, ModuleNameError> {
    name.parse::<ModuleName>().map(|m| m.file())
}

#[test]
fn names_scripts_build_by_joining_stay_inside_the_folder() {
    for (name, expected) in [
        ("hero_selection", "hero_selection.lua"),
        ("scripts/util/position", "scripts/util/position.lua"),
        ("scripts//util/x", "scripts/util/x.lua"),
        ("scripts/./util/x/", "scripts/util/x.lua"),
        (
            "scripts/hero/items/some-name",
            "scripts/hero/items/some-name.lua",
        ),
        ("dir/file.with.dots", "dir/file.with.dots.lua"),
        ("console", "console.lua"),
        ("com10", "com10.lua"),
    ] {
        assert_eq!(file(name).as_deref(), Ok(expected), "{name:?}");
    }
}

#[test]
fn names_that_could_leave_the_folder_are_refused() {
    for (name, refused) in [
        ("../secrets", ModuleNameError::Parent),
        ("scripts/../../x", ModuleNameError::Parent),
        ("/etc/passwd", ModuleNameError::Absolute),
        ("scripts/.hidden", ModuleNameError::Hidden),
        ("scripts/...", ModuleNameError::Hidden),
        ("scripts\\..\\x", ModuleNameError::Backslash),
        ("C:/x", ModuleNameError::Colon),
        ("scripts/x\0.txt", ModuleNameError::Control),
        ("scripts/x\n", ModuleNameError::Control),
        ("", ModuleNameError::Empty),
        ("././/", ModuleNameError::Empty),
        ("con", ModuleNameError::Device),
        ("lib/NUL.lua", ModuleNameError::Device),
        ("Com1", ModuleNameError::Device),
        ("lpt9.txt", ModuleNameError::Device),
        ("lib/aux", ModuleNameError::Device),
        ("lib/x.", ModuleNameError::Device),
        ("lib/x ", ModuleNameError::Device),
    ] {
        assert_eq!(file(name), Err(refused), "{name:?}");
    }
}

#[test]
fn names_are_bounded_in_length_and_depth() {
    let longest = "x".repeat(ModuleName::MAX_LENGTH);
    assert!(file(&longest).is_ok());
    assert_eq!(file(&format!("{longest}x")), Err(ModuleNameError::TooLong));
    let deepest = vec!["d"; ModuleName::MAX_COMPONENTS].join("/");
    assert!(file(&deepest).is_ok());
    assert_eq!(file(&format!("{deepest}/d")), Err(ModuleNameError::TooDeep));
}
