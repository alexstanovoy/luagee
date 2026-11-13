//! Counts, in tests only, the work done where a script chooses how much there is (slots scanned, bytes copied,
//! values moved), at the places that do it: the tests in `work/tests.rs` hold the counts against the fuel paid. Outside
//! tests `visit` does nothing and compiles away.

/// A kind of work and how many units of it one fuel pays for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Work {
    /// Table slots looked at.
    Slots,
    /// Work paid one fuel a unit: functions a native function calls, slots `next` steps over.
    Steps,
}

impl Work {
    /// The units one fuel pays for.
    #[cfg(test)]
    pub(crate) const fn per_fuel(self) -> u64 {
        match self {
            Work::Slots => crate::vm::VALUES_PER_FUEL as u64,
            Work::Steps => 1,
        }
    }
}

/// Every kind of work, in the order of its discriminant.
#[cfg(test)]
const ALL: [Work; 2] = [Work::Slots, Work::Steps];
#[cfg(test)]
const KINDS: usize = ALL.len();

#[cfg(test)]
std::thread_local! {
    static DONE: std::cell::Cell<[u64; KINDS]> = const { std::cell::Cell::new([0; KINDS]) };
}

/// Records `units` of `work` done.
#[inline(always)]
pub(crate) fn visit(work: Work, units: usize) {
    #[cfg(test)]
    DONE.with(|done| {
        let mut counts = done.get();
        counts[work as usize] += units as u64;
        done.set(counts);
    });
    #[cfg(not(test))]
    let _ = (work, units);
}

/// The work recorded since the last `take`, in fuel at each kind's price (rounded up), and in units.
#[cfg(test)]
pub(crate) fn take() -> (u64, u64) {
    let counts = DONE.with(|done| done.replace([0; KINDS]));
    let fuel = ALL
        .iter()
        .map(|&kind| counts[kind as usize].div_ceil(kind.per_fuel()))
        .sum();
    (fuel, counts.iter().sum())
}

#[cfg(test)]
mod tests;
