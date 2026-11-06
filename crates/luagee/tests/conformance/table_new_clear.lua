-- LuaJIT 2.1's table.new and table.clear, loaded with require as in LuaJIT.
print(table.new, table.clear)
local new = require "table.new"
local clear = require "table.clear"
print(type(new), type(clear), table.new == new, table.clear == clear, require "table.new" == new)
local t = new(10, 4)
print(type(t), #t, next(t))
for i = 1, 5 do t[i] = i end
t.x, t.y = "x", "y"
print(#t, t.x)
local u = new(0, 0)
u[1] = "one"
print(u[1], #u)
print(pcall(new))
print(pcall(new, 1))
print(pcall(new, "a", 1))
print(type(new(100000, 3)), type(new(0, 3)))
local big = {}
for i = 1, 100 do big[i] = i big["k" .. i] = i end
clear(big)
print(#big, next(big), big[1], big.k1)
big[3] = "again"
print(big[3], big[1])
-- Clearing while a loop walks the table: the loop goes on and ends.
local walked = {1, 2, 3, a = 1, b = 2}
local seen = 0
for k in pairs(walked) do
  seen = seen + 1
  clear(walked)
end
print(seen, next(walked))
print(pcall(clear, 1))
print(clear({}))
