-- expect: limit fuel_per_call
return string.find(("a"):rep(100), (".-"):rep(30) .. "b")
