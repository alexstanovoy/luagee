-- expect: error string length overflow
local t = {} local s = string.rep("y", 2^20) for i = 1, 1e5 do t[i] = s end return #table.concat(t)
