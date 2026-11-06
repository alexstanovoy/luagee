-- expect: limit fuel_per_call
local s = "xx" for i = 1, 64 do s = s:gsub("x", "xx") end return #s
