-- expect: error string length overflow
local t = {"x"} for i = 1, 40 do local s = table.concat(t) t = {s, s} end return #t
