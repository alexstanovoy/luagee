-- expect: error invalid value
local t = {} t[2^63 - 1024] = "x" print(#table.concat(t, "", 2^63 - 1024, math.huge))
