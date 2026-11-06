-- expect: ok true
local function f(n) if n == 0 then return 0 end return select(2, pcall(f, n - 1)) end print(f(1e5) ~= nil)
