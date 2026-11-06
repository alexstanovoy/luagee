-- expect: limit fuel_per_call
local function f() return pcall(f) end print((f()))
