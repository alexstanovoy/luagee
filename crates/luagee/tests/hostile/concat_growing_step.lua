-- expect: limit fuel_per_call
local s = "" while true do s = s .. string.rep("y", 1000) end
