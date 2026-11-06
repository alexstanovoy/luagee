-- expect: limit fuel_per_call
local t = {} for i = 1, 1e5 do table.insert(t, 1, i) end return #t
