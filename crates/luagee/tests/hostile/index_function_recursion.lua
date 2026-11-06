-- expect: error stack overflow
local t = setmetatable({}, {__index = function(t, k) return t.x end}) return t.x
