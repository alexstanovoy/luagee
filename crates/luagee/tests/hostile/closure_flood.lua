-- expect: limit fuel_per_call
local t = {} local i = 0 while true do i = i + 1 t[i] = function() return i end end
