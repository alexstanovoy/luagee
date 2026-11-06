-- expect: limit memory
local t = {} for i = 1, 1e9 do t[i] = string.rep("x", 100000 + i) end
