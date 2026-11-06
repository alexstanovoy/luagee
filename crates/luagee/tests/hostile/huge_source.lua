-- expect: error string length overflow
local f, e = loadstring("return " .. string.rep("1+", 5e7) .. "1") return f and f() or e
