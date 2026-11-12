use std::cell::Cell;

use super::Table;
use crate::Value;
use crate::heap::KeyHasher;

#[test]
fn dense_array_writes_do_not_evaluate_the_hash() {
    let mut table = Table::default();
    let hasher = KeyHasher::new([1, 2]);
    let hashes = Cell::new(0);
    for (key, value) in [(1.0, 10.0), (2.0, 20.0), (1.0, 30.0)] {
        table
            .set(
                Value::Number(key),
                || {
                    hashes.set(hashes.get() + 1);
                    hasher.number(key)
                },
                Value::Number(value),
                &hasher,
                &mut |_, _| Ok(()),
            )
            .expect("array write");
    }
    assert_eq!(hashes.get(), 0, "array storage never needs a key hash");
    assert_eq!(table.get_int(1, &hasher), Value::Number(30.0));
    assert_eq!(table.get_int(2, &hasher), Value::Number(20.0));
}

#[test]
fn sparse_numeric_writes_hash_once_and_still_migrate() {
    let mut table = Table::default();
    let hasher = KeyHasher::new([1, 2]);
    let hashes = Cell::new(0);
    for key in [3.0, 2.0, 1.0] {
        table
            .set(
                Value::Number(key),
                || {
                    hashes.set(hashes.get() + 1);
                    hasher.number(key)
                },
                Value::Number(key * 10.0),
                &hasher,
                &mut |_, _| Ok(()),
            )
            .expect("write");
    }
    assert_eq!(hashes.get(), 2, "the gap entries use the keyed hash part");
    assert_eq!(
        table.array(),
        &[
            Value::Number(10.0),
            Value::Number(20.0),
            Value::Number(30.0)
        ]
    );
}
