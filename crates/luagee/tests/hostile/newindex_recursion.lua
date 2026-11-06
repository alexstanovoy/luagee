-- expect: error stack overflow
local t = setmetatable({}, {__newindex = function(t, k, v) t[k] = v end}) t.x = 1
