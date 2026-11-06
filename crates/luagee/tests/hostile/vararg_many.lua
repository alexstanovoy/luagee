-- expect: error too many
local t = {} for i = 1, 1e5 do t[i] = i end local function f(...) return select("#", ...) end return f(unpack(t))
