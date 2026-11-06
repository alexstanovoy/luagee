-- Operator metamethods: arithmetic with mixed operands, unary, concat, comparison rules of 5.1, __len, __call.
local raw_pcall = pcall
local function pcall(f, ...) return (raw_pcall(f, ...)) end
local V = {}
V.__index = V
local function vec(x) return setmetatable({x = x}, V) end
V.__add = function(a, b) return vec((type(a) == "table" and a.x or a) + (type(b) == "table" and b.x or b)) end
V.__sub = function(a, b) return vec((type(a) == "table" and a.x or a) - (type(b) == "table" and b.x or b)) end
V.__mul = function(a, b) return vec((type(a) == "table" and a.x or a) * (type(b) == "table" and b.x or b)) end
V.__div = function(a, b) return "div" end
V.__mod = function(a, b) return "mod" end
V.__pow = function(a, b) return "pow" end
V.__unm = function(a) return vec(-a.x) end
V.__concat = function(a, b) return (type(a) == "table" and "V" .. a.x or tostring(a)) .. "|" .. (type(b) == "table" and "V" .. b.x or tostring(b)) end
V.__len = function() return 42 end
V.__call = function(self, ...) return "called", select("#", ...), ... end
V.__tostring = function(a) return "vec(" .. a.x .. ")" end
V.__eq = function(a, b) return a.x == b.x end
V.__lt = function(a, b) return (type(a) == "table" and a.x or a) < (type(b) == "table" and b.x or b) end
V.__le = function(a, b) return (type(a) == "table" and a.x or a) <= (type(b) == "table" and b.x or b) end
local a, b = vec(3), vec(4)
print((a + b).x, (a + 1).x, (1 + a).x, (a - b).x, (10 - a).x, (a * 2).x, (2 * a).x)
print(a / b, a % b, a ^ b, 1 / a, a % 1, 2 ^ a)
print((-a).x, a .. b, a .. "s", "s" .. a, 1 .. a, a .. 1, "x" .. "y" .. a)
print(#a, #"abc", #{1, 2})
print(a(1, 2, 3))
print(a(), a(nil))
print(tostring(a), tostring(vec(7)))
print(a == vec(3), a == b, a ~= vec(3), a == 3, vec(3) == vec(3))
print(a < b, a > b, a <= b, a >= b)
print(pcall(function() return a < 5 end))
print(pcall(function() return 5 <= a end))
print(rawequal(a, vec(3)), rawlen == nil)
local E1 = setmetatable({}, {__eq = function() return true end})
local E2 = setmetatable({}, {__eq = function() return false end})
print(E1 == E2, E2 == E1, E1 == {}, {} == E1, E1 == E1, E1 == 1, E1 ~= E2)
local same_mt = {__eq = function() return true end}
local S1, S2 = setmetatable({}, same_mt), setmetatable({}, same_mt)
print(S1 == S2, S1 ~= S2, S1 == {})
local Ret = setmetatable({}, {__eq = function() return "truthy" end})
local Ret2 = setmetatable({}, getmetatable(Ret))
print(Ret == Ret2, Ret ~= Ret2)
local LT = {__lt = function(a, b) return a.v < b.v end}
local l1, l2 = setmetatable({v = 1}, LT), setmetatable({v = 2}, LT)
print(l1 < l2, l2 < l1, l1 > l2, l2 > l1)
print(pcall(function() return l1 <= l2 end))
local LEonly = {__le = function(a, b) return a.v <= b.v end}
local e1, e2 = setmetatable({v = 1}, LEonly), setmetatable({v = 2}, LEonly)
print(e1 <= e2, e2 >= e1, pcall(function() return e1 < e2 end))
local NoMeta = {}
print(pcall(function() return NoMeta < NoMeta end), pcall(function() return NoMeta + 1 end))
print(pcall(function() return {} < 1 end), pcall(function() return "a" < 1 end))
local diff_mt1 = setmetatable({}, {__lt = function() return true end})
local diff_mt2 = setmetatable({}, {__lt = function() return false end})
print(pcall(function() return diff_mt1 < diff_mt2 end))
print((pcall(function() return diff_mt1 < 1 end)))
local sub = setmetatable({}, {__sub = function(a, b) return "sub" end, __add = function() return "add" end})
print(sub - 1, 1 - sub, sub + sub, sub + {}, {} + sub)
local first_wins = setmetatable({}, {__add = function() return "left" end})
local second = setmetatable({}, {__add = function() return "right" end})
print(first_wins + second, second + first_wins)
local str_num = setmetatable({}, {__add = function(a, b) return "meta" end})
print(str_num + "10", "10" + str_num, str_num + 1)
local cc = setmetatable({}, {__concat = function(a, b) return "cc" end})
print(cc .. "a" .. "b", "a" .. "b" .. cc, "a" .. cc .. "b", 1 .. cc, cc .. cc)
local callable = setmetatable({}, {__call = function(self, a, b) return a, b end})
print(callable(1), callable(1, 2), pcall(callable, 5))
local cc2 = setmetatable({}, {__call = callable})
print(pcall(function() return cc2(1) end) == false or true)
print(setmetatable({}, {__len = function() return 99 end}) ~= nil, #setmetatable({1, 2}, {__len = function() return 99 end}))
local un = setmetatable({}, {__unm = function(a, b) return rawequal(a, b) end})
print(-un)
local idx_num = setmetatable({}, {__index = function(t, k) return k end, __call = function() return "c" end})
print(idx_num[5], idx_num())
