-- expect: limit fuel_per_call
local t = {} for i = 1, 65 do t[i] = ("x"):rep(2^16) end while true do pcall(table.concat, t) end
