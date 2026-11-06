-- expect: limit fuel_per_call
print(xpcall(function() while true do end end, function() return "handled" end)) print("survived")
