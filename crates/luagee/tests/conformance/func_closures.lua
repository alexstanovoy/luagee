-- Closures and upvalues: shared upvalues, per-iteration loop variables, counters, closing over parameters.
local function counter()
  local n = 0
  return function() n = n + 1 return n end, function() return n end, function(v) n = v end
end
local inc, get, set = counter()
inc() inc()
print(get())
set(10)
print(inc(), get())
local inc2, get2 = counter()
print(inc2(), get2(), get())
local fns = {}
for i = 1, 3 do fns[i] = function() i = i + 1 return i end end
print(fns[1](), fns[1](), fns[2](), fns[3]())
local shared = {}
for i = 1, 3 do
  local x = i * 10
  shared[i] = {get = function() return x end, set = function(v) x = v end}
end
shared[1].set(99)
print(shared[1].get(), shared[2].get(), shared[3].get())
local function adder(a) return function(b) return function(c) return a + b + c end end end
print(adder(1)(2)(3))
local function make()
  local a, b = 1, 2
  local function swap() a, b = b, a end
  local function read() return a, b end
  return swap, read
end
local swap, read = make()
swap()
print(read())
local list = {}
local k = 1
while k <= 3 do
  local j = k
  list[k] = function() return j end
  k = k + 1
end
print(list[1](), list[2](), list[3]())
local ups = {}
do
  local x = 1
  ups[1] = function() x = x + 1 return x end
  local x = 100
  ups[2] = function() x = x + 1 return x end
end
print(ups[1](), ups[2](), ups[1]())
local function recursive_local()
  local function f(n) if n == 0 then return "done" end return f(n - 1) end
  return f(10)
end
print(recursive_local())
local self_ref = function(n) return n end
local self_ref = function(n) return self_ref(n) + 1 end
print(self_ref(1))
local function param_closure(a, b)
  return function() a = a + 1 return a + b end
end
local pc = param_closure(1, 10)
print(pc(), pc())
local function vararg_closure(...)
  local n = select("#", ...)
  local first = ...
  return function() return n, first end
end
print(vararg_closure("a", "b", "c")())
local fs = {}
for _, name in ipairs({"a", "b"}) do fs[name] = function() return name end end
print(fs.a(), fs.b())
local repeated = {}
local r = 0
repeat
  local v = r
  repeated[#repeated + 1] = function() return v end
  r = r + 1
until v >= 2
print(#repeated, repeated[1](), repeated[3]())
local gen = {}
for i = 1, 3 do
  gen[i] = coroutine.wrap(function() local c = i while true do coroutine.yield(c) c = c + 10 end end)
end
print(gen[1](), gen[1](), gen[3](), gen[2]())
local outer = 0
local function bump() outer = outer + 1 end
bump() bump()
print(outer)
local t = {}
function t.make() local hidden = 5 return function() hidden = hidden + 1 return hidden end end
local m1, m2 = t.make(), t.make()
print(m1(), m1(), m2())
