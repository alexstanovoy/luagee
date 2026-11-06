-- expect: ok true
print(tonumber(("1"):rep(1e6), 2) ~= nil)
