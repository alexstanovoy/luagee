-- expect: limit fuel_per_call
local mt = {} mt.__eq = function() while true do end end return setmetatable({}, mt) == setmetatable({}, mt)
