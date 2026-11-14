//! The README's example compiles, runs, and is the text in the README: its lines sit between the markers below.

use luagee::{Error, Keys, Limits, Lua};

const README: &str = include_str!("../../../README.md");
const THIS_FILE: &str = include_str!("readme_example.rs");

// Not reformatted: the README shows these lines as they are.
#[rustfmt::skip]
#[test]
fn readme_example_runs_as_written() -> Result<(), Error> {
    // README-BEGIN
    // Draw both from the host's entropy: scripts must never learn the hash key.
    let keys = Keys { hash: [0x1234_5678_9abc_def0, 0x0fed_cba9_8765_4321], random: 42 };
    let mut lua = Lua::new(Limits::DEFAULT, keys)?;
    let results = lua.exec("squares", b"local t = {} for i = 1, 3 do t[i] = i * i end return table.concat(t, ' ')")?;
    assert_eq!(lua.bytes(results[0]), Some(&b"1 4 9"[..]));

    // A runaway script runs out of fuel; the interpreter is still usable afterwards.
    lua.begin_tick();
    assert!(matches!(lua.exec("loop", b"while true do end"), Err(Error::Limit(_))));
    // README-END
    Ok(())
}

#[test]
fn readme_shows_the_tested_example_verbatim() {
    let marked = |text: &str, begin: &str, end: &str| -> String {
        let start = text.find(begin).expect("begin marker") + begin.len();
        let stop = text[start..].find(end).expect("end marker") + start;
        text[start..stop].to_owned()
    };
    let tested: String = marked(THIS_FILE, "// README-BEGIN\n", "    // README-END")
        .lines()
        .map(|line| format!("{}\n", line.strip_prefix("    ").unwrap_or(line)))
        .collect();
    let shown = marked(
        README,
        "```rust\nuse luagee::{Error, Keys, Limits, Lua};\n\n",
        "```",
    );
    assert_eq!(
        shown, tested,
        "the README example and the tested one differ"
    );
}
