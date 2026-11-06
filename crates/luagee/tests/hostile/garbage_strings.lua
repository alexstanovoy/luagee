-- expect: limit fuel_per_call
local i = 0 while true do i = i + 1 local s = string.rep("x", 10000) .. i end
