-- expect: error stack overflow
local function f(n) return 1 + f(n + 1) end return f(1)
