-- expect: error stack overflow
local mt = {} mt.__concat = function(a, b) return a .. b end return setmetatable({}, mt) .. "x"
