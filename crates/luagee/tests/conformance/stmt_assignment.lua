-- Local and multiple assignment: right-hand sides are evaluated before assigning, swaps, adjustment, shadowing.
local a, b, c = 1, 2
print(a, b, c)
a, b = b, a
print(a, b)
local x, y, z = (function() return 1, 2, 3 end)()
print(x, y, z)
local p, q = 1, 2, 3
print(p, q)
local t = {}
local i = 1
i, t[i] = i + 1, 20
print(i, t[1], t[2])
local u = {1, 2, 3}
local k = 1
k, u[k] = 2, "changed"
print(k, u[1], u[2])
local function f() return 1, 2 end
local r1, r2, r3 = f(), 10
print(r1, r2, r3)
r1, r2, r3 = 0, f()
print(r1, r2, r3)
r1, r2, r3 = f(), f()
print(r1, r2, r3)
r1, r2 = (f())
print(r1, r2)
local v = 1
do local v = v + 1; print(v) end
print(v)
local w = 5
local w = w * 2
print(w)
local function shadow() local print = 1; return print end
print(shadow())
g1, g2 = 1
print(g1, g2)
g1, g2 = nil, 2
print(g1, g2)
local s = {}
s.a, s.b, s.c = 1, 2
print(s.a, s.b, s.c)
local n = 0
local function bump() n = n + 1 return n end
local m = {}
m[bump()], m[bump()] = bump(), bump()
local keys = {}
for key, value in pairs(m) do keys[#keys + 1] = key .. "=" .. value end
table.sort(keys)
print(n, #keys)
local e1, e2 = nil
print(e1, e2)
local h1, h2 = ...
print(h1, h2, select("#", ...))
a, b, c = (function(...) return ... end)(1, nil, nil)
print(a, b, c)
local lt = {}
lt.x, lt.y = 1, 2
lt.x, lt.y = lt.y, lt.x
print(lt.x, lt.y)
