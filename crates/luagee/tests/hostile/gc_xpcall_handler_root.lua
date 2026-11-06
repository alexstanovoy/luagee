-- expect: ok false	handled
print(xpcall(function() collectgarbage() local t = {} for i = 1, 100 do t[i] = {} end collectgarbage() error("x") end, function(e) return "handled" end))
