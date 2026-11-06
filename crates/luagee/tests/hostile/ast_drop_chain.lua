-- expect: ok true
local f = loadstring("local a = 1 return a" .. ("+a"):rep(4e4)) print(f() == 40001)
