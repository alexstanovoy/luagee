-- expect: limit fuel_per_call
local s = ("x"):rep(2^21 + 1) while true do pcall(string.format, "%s%s", s, s) end
