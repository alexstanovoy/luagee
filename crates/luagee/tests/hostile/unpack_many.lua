-- expect: error too many
local t = {} for i = 1, 1e5 do t[i] = i end return select("#", unpack(t))
