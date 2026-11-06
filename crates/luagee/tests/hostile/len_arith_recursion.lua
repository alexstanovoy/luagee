-- expect: error stack overflow
local mt = {} mt.__add = function(a, b) return a + b end return setmetatable({}, mt) + 1
