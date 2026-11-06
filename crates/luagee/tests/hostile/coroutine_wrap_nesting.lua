-- expect: error stack overflow
local function nest(n) if n == 0 then return 0 end return coroutine.wrap(function() return nest(n - 1) + 1 end)() end return nest(1e5)
