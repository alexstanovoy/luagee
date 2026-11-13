use super::Arena;

std::thread_local! {
    static NUMERIC_HASHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn note_numeric_hash() {
    NUMERIC_HASHES.with(|count| count.set(count.get() + 1));
}

#[test]
fn vm_dense_numeric_writes_do_not_hash_array_keys() {
    let mut lua = crate::Lua::new(
        crate::Limits::DEFAULT,
        crate::Keys {
            hash: [193, 271],
            random: 0,
        },
    )
    .expect("state");
    let function = lua
        .load("array", b"local t={} for i=1,1000 do t[i]=i end")
        .expect("compile");
    NUMERIC_HASHES.with(|count| count.set(0));
    lua.call(function, &[]).expect("dense writes");
    assert_eq!(
        NUMERIC_HASHES.with(std::cell::Cell::get),
        0,
        "the production heap write path defers numeric hashing"
    );
}

#[test]
fn exhausted_generations_retire_the_slot_instead_of_reusing_old_handles() {
    let mut arena = Arena::default();
    let original = arena.insert(17);
    arena.begin_collection();
    arena.sweep(usize::MAX, |_| 0, |_, _| {});
    arena.end_sweep(0);
    arena.slots[original.index() as usize].generation = u32::MAX - 1;
    let last = arena.insert(29);
    assert_eq!(last.generation(), u32::MAX);
    arena.begin_collection();
    arena.sweep(usize::MAX, |_| 0, |_, _| {});
    arena.end_sweep(0);
    let replacement = arena.insert(41);
    assert_ne!(last.index(), replacement.index());
    assert!(arena.try_get(last).is_none());
}
