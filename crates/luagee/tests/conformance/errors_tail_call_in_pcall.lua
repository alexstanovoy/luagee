-- A function called by pcall or xpcall that tail-calls another stays protected: the callee's error is pcall's.
local function bad() error("boom") end
print(pcall(function() return bad() end))
print(xpcall(function() return bad() end, function(m) return "handled: " .. m end))
print(pcall(function() return (function() return bad() end)() end))
local depth = 0
local function down(n) if n == 0 then error("bottom") end return down(n - 1) end
print(pcall(down, 100))
print(pcall(function() return down(5) end))
print(select("#", pcall(function() return 1, 2, 3 end)))
print(pcall(function() return select(2, "a", "b") end))
