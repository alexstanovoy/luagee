-- expect: limit fuel_per_call
local t = {} for i = 1, 1e5 do t[i] = i end for i = 1, 1e5 do table.remove(t, 1) end return #t
