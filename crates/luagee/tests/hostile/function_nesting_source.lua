-- expect: ok true
local f, e = loadstring("return " .. ("function() return "):rep(1e4) .. "1" .. (" end"):rep(1e4)) print(f == nil and e:find("too many syntax levels") ~= nil)
