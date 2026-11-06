-- expect: limit memory
local f = loadstring("return a" .. ("+a"):rep(3e5)) print(f ~= nil)
