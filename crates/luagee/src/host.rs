//! Operations for embedding callbacks. Allocation never runs script code or collects; reentry through `call` can.

use crate::{Args, Limit, LuaError, TableId, Value, Vm};

impl Args {
    /// The number of arguments, including explicit nils. Argument indexes start at zero.
    #[must_use]
    pub fn len(self) -> usize {
        self.count
    }

    /// Whether the callback was called without arguments.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
}

impl Vm {
    /// The bytes of a string value, or None for a collected handle. Values belong to this interpreter.
    #[must_use]
    pub fn bytes(&self, value: Value) -> Option<&[u8]> {
        match value {
            Value::String(string) => self.heap.strings.try_get(string.0).map(|string| string.bytes.as_ref()),
            _ => None,
        }
    }

    /// An empty table. Pin it before calling back into Lua if Lua cannot otherwise reach it.
    pub fn new_table(&mut self) -> Result<TableId, LuaError> {
        self.charge_fuel(crate::vm::ALLOCATION_FUEL)?;
        self.heap.new_table(0, 0)
    }

    /// A table lookup without metamethods. Collected handles raise Lua errors; handles belong to this interpreter.
    pub fn raw_get(&mut self, table: TableId, key: Value) -> Result<Value, LuaError> {
        self.charge_fuel(1)?;
        self.validate(Value::Table(table))?;
        self.validate(key)?;
        Ok(self.heap.raw_get(table, key))
    }

    /// A table write without metamethods, behind the collector's barrier. Nil and NaN keys raise Lua errors.
    /// Pays a constant lookup cost and for storage actually added, so sequential list writes stay linear.
    pub fn raw_set(&mut self, table: TableId, key: Value, value: Value) -> Result<(), LuaError> {
        self.validate(Value::Table(table))?;
        self.validate(key)?;
        self.validate(value)?;
        self.charge_fuel(1)?;
        let before = self.heap.table(table).bytes();
        self.raw_set_checked(table, key, value)?;
        let growth = self.heap.table(table).bytes().saturating_sub(before);
        self.charge_fuel(growth as u64 / 16)
    }

    /// Sets a table's metatable behind the collector's barrier. The host can replace a protected metatable.
    /// Use a non-nil `__metatable` field to hide a private metatable from scripts.
    pub fn set_metatable(
        &mut self,
        table: TableId,
        metatable: Option<TableId>,
    ) -> Result<(), LuaError> {
        self.charge_fuel(1)?;
        self.validate(Value::Table(table))?;
        if let Some(meta) = metatable { self.validate(Value::Table(meta))?; }
        self.heap.set_metatable(table, metatable);
        Ok(())
    }

    /// An opaque, typed host object. Its inline size and `retained_bytes` count against memory, including after GC.
    /// `retained_bytes` must cover allocations owned by `data` beyond its inline size (for example a Vec's capacity).
    /// Host code must bound such allocations before constructing `data`; callbacks are trusted Rust code.
    /// A payload's embedded Lua handles are not traced: pin them separately, as for handles captured by callbacks.
    pub fn userdata<T: 'static>(
        &mut self,
        data: T,
        metatable: Option<TableId>,
        retained_bytes: usize,
    ) -> Result<Value, LuaError> {
        self.charge_fuel(crate::vm::ALLOCATION_FUEL)?;
        if let Some(meta) = metatable { self.validate(Value::Table(meta))?; }
        let bytes = std::mem::size_of::<T>()
            .checked_add(retained_bytes)
            .ok_or(LuaError::Limit(Limit::Memory))?;
        Ok(Value::Userdata(self.heap.new_userdata_charged(
            Box::new(data),
            metatable,
            bytes,
        )?))
    }

    /// Reads an opaque object by its Rust type, independent of any script-visible metatable.
    /// A table with a copied metatable cannot impersonate an object. Collected handles return None.
    #[must_use]
    pub fn userdata_ref<T: 'static>(&self, value: Value) -> Option<&T> {
        match value {
            Value::Userdata(id) => self.heap.userdata.try_get(id.0)?.data.downcast_ref(),
            _ => None,
        }
    }

    /// Mutably borrows typed host data; wrong types and collected handles return None.
    /// The borrow prevents reentry through this VM until it ends. Mutation must stay within the payload's
    /// declared retained allocation; handles stored in the payload still need independent roots.
    #[must_use]
    pub fn userdata_mut<T: 'static>(&mut self, value: Value) -> Option<&mut T> {
        match value {
            Value::Userdata(id) => self.heap.userdata.try_get_mut(id.0)?.data.downcast_mut(),
            _ => None,
        }
    }

    /// Keeps a live value reachable until one matching `unpin`. Duplicate pins are independent.
    /// Pin callback captures, temporary values and nested results before reentering Lua. Storage is memory charged.
    pub fn pin(&mut self, value: Value) -> Result<(), LuaError> {
        self.charge_fuel(1)?;
        self.validate(value)?;
        if self.pinned.len() == self.pinned.capacity() {
            let growth = self.pinned.capacity().max(4);
            self.charge_fuel(1 + self.pinned.len() as u64 / 8)?;
            self.heap.charge(
                growth
                    .checked_mul(16)
                    .ok_or(LuaError::Limit(Limit::Memory))?,
            )?;
            self.pinned.reserve_exact(growth);
        }
        self.heap.barrier(value);
        self.pinned.push(value);
        Ok(())
    }

    /// Releases one matching root; false if absent. The scan pays fuel, but cleanup still happens if fuel ran out.
    pub fn unpin(&mut self, value: Value) -> Result<bool, LuaError> {
        let charged = self.charge_fuel(1 + self.pinned.len() as u64 / 8);
        let found = self.pinned.iter().rposition(|v| {
            *v == value || matches!((*v, value), (Value::Number(a), Value::Number(b)) if a.is_nan() && b.is_nan())
        });
        if let Some(index) = found {
            self.pinned.swap_remove(index);
        }
        charged.map(|()| found.is_some())
    }

    /// Calls Lua from a callback under its current fuel, tick and native recursion bounds; no budget is refilled.
    /// Failed calls restore the caller's stack and frames, so a callback may handle a runtime error and continue.
    /// Only callable inside a host callback; top-level callers use `Lua::call` for tick accounting.
    /// Propagate `LuaError::Limit` unchanged. Results and runtime error values stay rooted until this callback ends.
    /// Pin any value retained beyond the callback. Expired `Args` cannot read another callback's frame.
    pub fn call(&mut self, function: Value, args: &[Value]) -> Result<Vec<Value>, LuaError> {
        if self.host_depth == 0 { return Err(self.error("Vm::call requires an active host callback; use Lua::call at top level")); }
        let slot = self.stack.len();
        let depth = self.frames.len();
        let top = self.top;
        let result = (|| {
            self.charge_fuel(1 + args.len() as u64 / crate::vm::VALUES_PER_FUEL as u64)?;
            self.validate(function)?;
            for &value in args { self.validate(value)?; }
            self.push(function)?;
            for &value in args {
                self.push(value)?;
            }
            let count = self.call_value(slot, args.len())?;
            self.charge_fuel(1 + count as u64 / 8)?;
            Ok(self.stack[slot..slot + count].to_vec())
        })();
        let result = result.map_err(|error| self.locate(error, None));
        self.frames.truncate(depth);
        self.top = top;
        if result.is_err() { self.stack.truncate(slot); }
        if let Err(LuaError::Runtime(thrown)) = &result {
            self.push(thrown.value)?;
        }
        // Successful results stay on the stack as temporary roots until call_native returns.
        result
    }

    pub(crate) fn args_are_live(&self, args: Args) -> bool {
        args.serial != 0 && args.serial == self.active_serial
            && args.base.checked_add(args.count).is_some_and(|end| end <= self.stack.len())
    }

    pub(crate) fn check_args(&mut self, args: Args) -> Result<(), LuaError> {
        if self.args_are_live(args) { Ok(()) } else { Err(self.error("expired callback arguments")) }
    }

    /// Rejects collected or reused handles before they enter a script's stack or heap.
    pub(crate) fn validate(&mut self, value: Value) -> Result<(), LuaError> {
        let live = match value {
            Value::Nil | Value::Boolean(_) | Value::Number(_) => true,
            Value::String(id) => self.heap.strings.try_get(id.0).is_some(),
            Value::Table(id) => self.heap.tables.try_get(id.0).is_some(),
            Value::Function(id) => self.heap.functions.try_get(id.0).is_some(),
            Value::Userdata(id) => self.heap.userdata.try_get(id.0).is_some(),
            Value::Thread(id) => self.threads.try_get(id.0).is_some(),
            Value::Cell(id) => self.heap.cells.try_get(id.0).is_some(),
        };
        if live { Ok(()) } else { Err(self.error("stale interpreter handle")) }
    }
}
