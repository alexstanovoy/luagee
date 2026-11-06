-- expect: limit memory
local t = {} for i = 1, 1e9 do local x = i t[i] = function() return x end end
