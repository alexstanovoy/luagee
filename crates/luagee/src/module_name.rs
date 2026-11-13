//! The names `require` and `dofile` take, and the rule that keeps them inside the script folder.

use std::fmt;
use std::str::FromStr;

/// A module's name, checked to stay inside the folder the host loads scripts from.
///
/// `require` and `dofile` never take a path: the host gets a `ModuleName` and reads `<folder>/<name>.lua` itself, so a
/// script cannot name a file outside the folder whatever string it builds. A name is components joined by `/`; empty
/// components and `.` are dropped (`a//b/./c` is `a/b/c`), so the names scripts build by joining (`dir .. "/util/x"`)
/// keep working. Anything that could leave the folder or mean something else on another system is refused rather
/// than cleaned up: `..`, a leading `/`, `\`, `:`, a component starting with `.`, control characters.
///
/// `require` turns `.` into `/` first, as Lua 5.1 does (`require "util.x"` is `util/x`); `dofile` does not.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModuleName {
    /// The components joined by `/`, without empty or `.` components.
    name: String,
}

/// Why a string is not a [`ModuleName`]. More reasons may be added: match with a wildcard arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
#[non_exhaustive]
pub enum ModuleNameError {
    /// It has no component.
    Empty,
    /// It is longer than [`ModuleName::MAX_LENGTH`] bytes.
    TooLong,
    /// It has more than [`ModuleName::MAX_COMPONENTS`] components.
    TooDeep,
    /// It starts with `/`, a path from the root of the file system.
    Absolute,
    /// A component is `..`, the folder above.
    Parent,
    /// A component starts with `.`, which names hidden files and can be read as `..` by some systems.
    Hidden,
    /// It has `\`, a separator on some systems and an escape on others.
    Backslash,
    /// It has `:`, a drive or stream on some systems.
    Colon,
    /// It has a control character, NUL among them.
    Control,
    /// A component is a device name on Windows (`CON`, `NUL`, `COM1`, `LPT1`, ... with any extension), or ends with a
    /// dot or a space, which Windows strips: either names something other than a file in the folder.
    Device,
}

impl ModuleName {
    /// The longest name, in bytes.
    pub const MAX_LENGTH: usize = 256;
    /// The most components a name may have.
    pub const MAX_COMPONENTS: usize = 16;

    /// The components, outermost folder first.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.name.split('/')
    }

    /// The file the host reads, relative to its script folder: the name and `.lua`.
    #[must_use]
    pub fn file(&self) -> String {
        format!("{}.lua", self.name)
    }
}

impl FromStr for ModuleName {
    type Err = ModuleNameError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if name.len() > Self::MAX_LENGTH {
            return Err(ModuleNameError::TooLong);
        }
        if name.starts_with('/') {
            return Err(ModuleNameError::Absolute);
        }
        if let Some(refused) = name.chars().find_map(refusal) {
            return Err(refused);
        }
        let mut components = Vec::new();
        for component in name.split('/').filter(|c| !c.is_empty() && *c != ".") {
            if component == ".." {
                return Err(ModuleNameError::Parent);
            }
            if component.starts_with('.') {
                return Err(ModuleNameError::Hidden);
            }
            if is_device(component) {
                return Err(ModuleNameError::Device);
            }
            components.push(component);
        }
        if components.is_empty() {
            return Err(ModuleNameError::Empty);
        }
        if components.len() > Self::MAX_COMPONENTS {
            return Err(ModuleNameError::TooDeep);
        }
        Ok(ModuleName {
            name: components.join("/"),
        })
    }
}

/// Whether Windows would read a component as a device, or change it by stripping a trailing dot or space.
fn is_device(component: &str) -> bool {
    if component.ends_with(['.', ' ']) {
        return true;
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
    };
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered("COM")
        || numbered("LPT")
}

fn refusal(c: char) -> Option<ModuleNameError> {
    match c {
        '\\' => Some(ModuleNameError::Backslash),
        ':' => Some(ModuleNameError::Colon),
        c if c.is_control() => Some(ModuleNameError::Control),
        _ => None,
    }
}

impl fmt::Display for ModuleName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

impl fmt::Display for ModuleNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleNameError::Empty => f.write_str("a module name needs at least one component"),
            ModuleNameError::TooLong => write!(
                f,
                "a module name may have {} bytes at most",
                ModuleName::MAX_LENGTH
            ),
            ModuleNameError::TooDeep => write!(
                f,
                "a module name may have {} components at most",
                ModuleName::MAX_COMPONENTS
            ),
            ModuleNameError::Absolute => f.write_str(
                "a module name is relative to the script folder: it may not start with '/'",
            ),
            ModuleNameError::Parent => {
                f.write_str("a module name may not name the folder above ('..')")
            }
            ModuleNameError::Hidden => {
                f.write_str("a module name's components may not start with '.'")
            }
            ModuleNameError::Backslash => {
                f.write_str("a module name separates its components with '/', never '\\'")
            }
            ModuleNameError::Colon => f.write_str("a module name may not have ':'"),
            ModuleNameError::Control => {
                f.write_str("a module name may not have control characters")
            }
            ModuleNameError::Device => f.write_str(
                "a module name's components may not be device names or end with a dot or a space",
            ),
        }
    }
}

impl std::error::Error for ModuleNameError {}

#[cfg(test)]
mod tests;
