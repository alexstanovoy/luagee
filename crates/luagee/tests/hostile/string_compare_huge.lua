-- expect: limit fuel_per_call
local s = ("a"):rep(4e6) local t = s .. "b" while true do local _ = s < t end
