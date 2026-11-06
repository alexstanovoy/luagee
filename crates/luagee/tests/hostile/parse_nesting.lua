-- expect: limit module_size
local f, e = loadstring(string.rep("(", 1e6) .. "1" .. string.rep(")", 1e6)) return f and f() or e
