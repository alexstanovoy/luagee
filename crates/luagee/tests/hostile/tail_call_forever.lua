-- expect: limit fuel_per_call
local function f() return f() end return f()
