-- expect: limit fuel_per_call
local f = assert(loadstring(("do local a end "):rep(3e4) .. "g()"))
while true do pcall(f) end
