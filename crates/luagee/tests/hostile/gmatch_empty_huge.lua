-- expect: limit fuel_per_call
local s = ("x"):rep(4e6) local n = 0 for _ in s:gmatch("") do n = n + 1 end return n
