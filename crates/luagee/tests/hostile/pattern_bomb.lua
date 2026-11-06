-- expect: limit fuel_per_call
return string.find(string.rep("a", 40), string.rep("a*", 40) .. "b")
