-- expect: ok false	X
print(pcall(setmetatable, "x", {}), ("x"):upper())
