-- expect: limit fuel_per_call
local s = ("a"):rep(2e6) .. "c" while true do local _ = s:find("^(a*)b%1") end
