-- expect: error string length overflow
local s = ("x"):rep(3000) return #s:gsub(".", s)
