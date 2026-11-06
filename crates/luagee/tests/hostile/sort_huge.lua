-- expect: limit memory
local t = {} for i = 1, 3e6 do t[i] = -i end table.sort(t) return #t
