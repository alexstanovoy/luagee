-- expect: ok true
local f, e = loadstring("return " .. ("'a'.."):rep(1e5) .. "'a'") print(f == nil and e:find("too many syntax levels") ~= nil)
