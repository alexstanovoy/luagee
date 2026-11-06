-- expect: ok false	true
local t = {} t.self = t local ok, e = pcall(error, t) print(ok, e == t)
