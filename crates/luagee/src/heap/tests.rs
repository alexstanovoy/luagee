use super::Arena;

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
