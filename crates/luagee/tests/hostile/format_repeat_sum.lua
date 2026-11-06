-- expect: error string length overflow
local s = ("x"):rep(2^20) local t = string.format("%s%s%s%s%s", s, s, s, s, s) return #t
