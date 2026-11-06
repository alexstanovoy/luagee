-- Varargs: ..., select('#'), select(n), trailing nils, {...}, table.pack-free idioms, adjustment in every context.
local function count(...) return select("#", ...) end
print(count(), count(nil), count(nil, nil), count(1, nil), count(nil, 1), count((count())))
local function pass(...) return ... end
print(pass(), pass(1), pass(1, 2, 3))
print((pass(1, 2, 3)))
print(pass(1, 2), pass(3, 4))
print(pass(1, 2), (pass(3, 4)))
print(count(pass(1, 2), pass(3, 4)), count(pass(), pass()), count((pass())))
print(select(2, "a", "b", "c"), (select(2, "a", "b", "c")))
print(select(-1, "a", "b", "c"), select(-2, "a", "b", "c"))
print(select("#"), select("#", nil), select("#", nil, nil, nil))
print(pcall(select, 0, "a") == false, pcall(select, -5, "a") == false)
print(select(5, "a", "b"))
print(select(3, "a", "b", "c"))
local t = {pass(1, 2, 3)}
print(#t)
t = {pass(1, 2, 3), 10}
print(#t, t[1], t[2])
t = {10, pass(1, 2, 3)}
print(#t)
t = {(pass(1, 2, 3))}
print(#t)
t = {pass()}
print(#t)
local function tail_nils(...) local a = {...} return #a, select("#", ...) end
print(tail_nils(1, 2, 3))
print(tail_nils())
local function first(a, ...) return a, select("#", ...) end
print(first(1, 2, 3), first(), first(nil, nil))
local function named(a, b, ...) return a, b, ... end
print(named(1), named(1, 2), named(1, 2, 3, 4))
print(count(unpack({1, 2, 3})), count(unpack({1, nil, 3}, 1, 3)), count(unpack({}, 1, 2)))
local function sum(...)
  local s = 0
  for i = 1, select("#", ...) do s = s + (select(i, ...) or 0) end
  return s
end
print(sum(1, 2, 3, nil, 5), sum())
local function forward(...) return count(...), count(..., "x"), count("x", ...) end
print(forward(1, 2, 3))
local function middle(...) local a, b = ..., "last" return a, b end
print(middle(1, 2, 3))
local function many(n) local t = {} for i = 1, n do t[i] = i end return unpack(t) end
print(count(many(200)), (select(200, many(200))))
print(count(many(0)), count(many(1)))
print(string.format("%s %s", pass("a", "b", "c")))
print(pcall(string.format, "%s %s", pass("a")) == false)
print(math.max(pass(3, 9, 4)), math.min(pass(3, 9, 4)))
local function va_in_closure(...)
  local args = {...}
  return function() return unpack(args) end
end
print(va_in_closure(7, 8, 9)())
print(loadstring("local a, b = ... return b, a")(1, 2))
print(select("#", loadstring("return ...")(nil, nil)))
print((function(...) return select("#", ...) end)(nil, nil, nil))
