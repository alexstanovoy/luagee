-- expect: ok true
local f, e = loadstring("return " .. ("{"):rep(1e5) .. ("}"):rep(1e5)) print(f == nil and e:find("too many syntax levels") ~= nil)
