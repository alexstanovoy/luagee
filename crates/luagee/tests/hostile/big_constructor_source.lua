-- expect: limit module_size
local f, e = loadstring("return {" .. ("1,"):rep(1e6) .. "}") return f and #f() or e
