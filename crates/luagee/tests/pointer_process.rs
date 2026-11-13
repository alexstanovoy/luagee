//! Pointer text is a deterministic local identity, including across fresh host processes.
#![cfg(not(target_arch = "wasm32"))]

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use luagee::{Keys, Limits, Lua};

fn pointer_snapshot() -> Vec<u8> {
    let mut lua = Lua::new(
        Limits::DEFAULT,
        Keys {
            hash: [1, 2],
            random: 3,
        },
    )
    .expect("interpreter");
    // Userdata starts in an empty arena: type 4, generation 1, slots 0 and 1. These are policy constants,
    // not expectations recorded from the formatter, and cannot be host addresses or interpreter owner ids.
    let first = lua.vm().userdata(1u8, None, 0).expect("first object");
    lua.pin(first).expect("root");
    let second = lua.vm().userdata(2u8, None, 0).expect("second object");
    lua.pin(second).expect("root");
    lua.set_global("first", first).expect("global");
    lua.set_global("second", second).expect("global");
    let output = Rc::new(RefCell::new(Vec::new()));
    let sink = output.clone();
    lua.set_print(Box::new(move |bytes| {
        sink.borrow_mut().extend_from_slice(bytes)
    }));
    lua.exec(
        "pointer_snapshot",
        br#"
        assert(string.format('%p',first)=='0x40000000100000000')
        assert(string.format('%p',second)=='0x40000000100000001')
        print(string.format('%p',first),string.format('%p',second))
        local values={{},function() end,coroutine.create(function() end),'probe',print}
        local tags={'2','3','5','1','3'}
        for i,value in ipairs(values) do
            local pointer=string.format('%p',value)
            assert(#pointer==19 and pointer:sub(1,3)=='0x'..tags[i])
            assert(pointer:match('^0x%x+$'))
            print(pointer)
        end
    "#,
    )
    .expect("exact identities and shapes");
    output.take()
}

#[test]
fn pointer_identities_match_exact_local_values_across_processes() {
    if std::env::var_os("LUAGEE_POINTER_TEST_CHILD").is_some() {
        let snapshot = pointer_snapshot();
        print!(
            "SNAPSHOT_BEGIN\n{}SNAPSHOT_END\n",
            String::from_utf8(snapshot).expect("ASCII ids")
        );
        return;
    }
    let expected = pointer_snapshot();
    assert_eq!(
        expected,
        pointer_snapshot(),
        "different interpreter owners do not affect ids"
    );
    for _ in 0..2 {
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "pointer_identities_match_exact_local_values_across_processes",
                "--nocapture",
            ])
            .env("LUAGEE_POINTER_TEST_CHILD", "1")
            .output()
            .expect("fresh process");
        assert!(
            output.status.success(),
            "child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("ASCII child output");
        let (_, snapshot) = text.split_once("SNAPSHOT_BEGIN\n").expect("snapshot start");
        let (snapshot, _) = snapshot.split_once("SNAPSHOT_END\n").expect("snapshot end");
        assert_eq!(snapshot.as_bytes(), expected);
    }
}
