-- expect: error stack overflow
local mt = {} mt.__lt = function(a, b) return a < b end return setmetatable({}, mt) < setmetatable({}, mt)
