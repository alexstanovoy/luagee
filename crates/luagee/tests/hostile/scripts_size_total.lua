-- expect: limit fuel_per_call
local source = "return 1" .. (" "):rep(2^20 - 16) for i = 1, 40 do loadstring(source) end
