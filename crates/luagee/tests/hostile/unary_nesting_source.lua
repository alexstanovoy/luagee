-- expect: limit module_size
local f, e = loadstring("return " .. ("- "):rep(1e6) .. "1") return f and f() or e
