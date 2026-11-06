-- expect: limit fuel_per_call
local s = ("1"):rep(3e6) while true do local _ = pcall(function() return s + 0 end) end
