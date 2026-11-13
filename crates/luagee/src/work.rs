//! Counts, in tests only, the work done where a script chooses how much there is (slots scanned, bytes copied,
//! values moved), at the places that do it: the tests in `work/tests.rs` hold the counts against the fuel paid. Outside
//! tests `visit` does nothing and compiles away.

/// A kind of work; `work/tests.rs` knows how many units of each one fuel pays for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Work {
    /// Table slots looked at.
    Slots,
    /// Bytes copied or hashed.
    Bytes,
    /// Values the collector looks at.
    Collector,
    /// Work paid one fuel a unit: functions a native function calls, slots `next` steps over.
    Steps,
}

/// Records `units` of `work` done.
#[inline(always)]
pub(crate) fn visit(work: Work, units: usize) {
    #[cfg(test)]
    tests::record(work, units);
    #[cfg(not(test))]
    let _ = (work, units);
}

#[cfg(test)]
mod tests;
