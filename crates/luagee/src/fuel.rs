//! The fuel a call has left. The heap holds it, so that the heap's own work (growing an arena, scanning a table) is
//! paid before it is done, wherever the heap is called from.

use crate::{Limit, LuaError};

/// The fuel left and which limit runs out with it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Fuel {
    pub(crate) left: u64,
    /// Which limit runs out when `left` does.
    pub(crate) limit: Limit,
}

impl Fuel {
    /// Spends `amount`, or spends everything and fails when less is left.
    #[inline]
    pub(crate) fn spend(&mut self, amount: u64) -> Result<(), LuaError> {
        if self.left < amount {
            return Err(self.exhaust());
        }
        self.left -= amount;
        Ok(())
    }

    /// The units of work, at `per_fuel` units a fuel, that the fuel left pays for.
    #[inline]
    pub(crate) fn affordable(&self, per_fuel: usize) -> usize {
        usize::try_from(self.left).map_or(usize::MAX, |left| left.saturating_mul(per_fuel))
    }

    /// Ends the call: nothing is left.
    pub(crate) fn exhaust(&mut self) -> LuaError {
        self.left = 0;
        LuaError::Limit(self.limit)
    }
}
