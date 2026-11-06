-- expect: ok true
local f, e = loadstring(("do "):rep(1e5) .. ("end "):rep(1e5)) print(f == nil and e:find("too many syntax levels") ~= nil)
