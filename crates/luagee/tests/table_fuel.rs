use luagee::{Error, Keys, Limit, Limits, Lua};

// Prices recorded independently from 9f8cdd8 before applying the storage optimization.
#[test]
fn table_move_keeps_new_base_prices_for_each_storage_path() {
    for (source, minimum) in [
        ("local t={1,2,3,4};table.move(t,1,3,2)", 34),
        (
            "local a={} for i=1,1000 do a[i]=i end local b={} table.move(a,1,1000,1,b)",
            7297,
        ),
        ("local a={10,20,30};local b={};table.move(a,1,3,-3,b)", 44),
        ("local t={1,2};table.move({},1,3,3,t)", 33),
    ] {
        for fuel in [minimum - 1, minimum] {
            let mut lua = Lua::new(
                Limits::DEFAULT
                    .with_fuel_per_call(fuel)
                    .with_fuel_per_tick(fuel),
                Keys {
                    hash: [193, 271],
                    random: 0,
                },
            )
            .expect("state");
            let result = lua.exec("move", source.as_bytes());
            if fuel == minimum {
                assert!(result.is_ok(), "{source}: {result:?}");
            } else {
                assert!(
                    matches!(result, Err(Error::Limit(Limit::FuelPerCall))),
                    "{source}: {result:?}"
                );
            }
            assert_eq!(lua.tick_fuel_left(), 0);
        }
    }
}

// Recorded from the approved conformance base 9f8cdd8, before numeric write hashing changed. The successful budget is exact;
// one unit less must refuse. These are prices, independent of wall time and hash bucket layout.
#[test]
fn table_workloads_keep_their_preoptimization_fuel_prices() {
    for (source, minimum) in [
        ("local t={} for i=1,1000 do t[i]=i end", 4147),
        ("local t={} for i=1,1000 do t[#t+1]=i end", 6147),
        (
            "local t={} for i=1,1000 do t[i]=i end for i=1,1000 do t[i]=nil end",
            7151,
        ),
        // Moving 999 keys into the array looks each up twice (planning, then moving): one fuel more a key since
        // the second lookup is paid, 5316 before.
        ("local t={} for i=2,1000 do t[i]=i end t[1]=1", 6315),
    ] {
        for fuel in [minimum - 1, minimum] {
            let mut lua = Lua::new(
                Limits::DEFAULT
                    .with_fuel_per_call(fuel)
                    .with_fuel_per_tick(fuel),
                Keys {
                    hash: [193, 271],
                    random: 0,
                },
            )
            .expect("state");
            let result = lua.exec("priced", source.as_bytes());
            if fuel == minimum {
                assert!(result.is_ok(), "{source}: {result:?}");
            } else {
                assert!(
                    matches!(result, Err(Error::Limit(Limit::FuelPerCall))),
                    "{source}: one unit below the recorded price: {result:?}"
                );
            }
            assert_eq!(lua.tick_fuel_left(), 0);
        }
    }
}
