-- expect: error stack overflow
local t = setmetatable({}, {}) getmetatable(t).__tostring = function(s) return tostring(s) end return tostring(t)
