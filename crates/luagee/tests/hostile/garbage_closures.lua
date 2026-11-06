-- expect: limit fuel_per_call
local i = 0 while true do i = i + 1 local f = function() return i end end
