-- get/setmetatable, __metatable protection, __tostring, __mode presence, string metatable, metatable per type.
local mt = {}
local t = setmetatable({}, mt)
print(getmetatable(t) == mt, getmetatable({}), getmetatable(1), getmetatable(nil), getmetatable(print))
print(setmetatable(t, nil) == t, getmetatable(t))
local protected = setmetatable({}, {__metatable = "locked"})
print(getmetatable(protected), pcall(setmetatable, protected, {}))
local fake = setmetatable({}, {__metatable = false})
print(getmetatable(fake), pcall(setmetatable, fake, nil) == false)
print(pcall(setmetatable, 1, {}) == false, pcall(setmetatable, {}, 1) == false, pcall(setmetatable, {}) == false)
print((pcall(setmetatable, {}, nil)))
print(select("#", setmetatable({}, nil)))
local named = setmetatable({}, {__tostring = function() return "I am named" end})
print(tostring(named), #tostring(named))
print((tostring(setmetatable({}, {__tostring = function() return "x" end}))))
print(pcall(tostring, setmetatable({}, {__tostring = function() return 42 end})))
print(pcall(tostring, setmetatable({}, {__tostring = function() return nil end})) )
print(tostring(nil), tostring(true), tostring(false), tostring(12), tostring(-1.5), tostring("s"), tostring(1e100))
print(type(tostring({})), tostring({}):sub(1, 6), tostring(print):sub(1, 9))
print(getmetatable("").__index == string, ("x").rep == string.rep)
local smt = getmetatable("")
print(type(smt), type(smt.__index))
string.shout = function(s) return s:upper() .. "!" end
print(("hey"):shout())
string.shout = nil
print(pcall(function() return ("hey"):shout() end) == false)
local weak = setmetatable({}, {__mode = "k"})
weak[{}] = 1
local weakv = setmetatable({}, {__mode = "v"})
weakv[1] = {}
local weakkv = setmetatable({}, {__mode = "kv"})
weakkv.x = 1
print(type(weak), type(weakv), weakkv.x)
local gc = setmetatable({}, {__gc = function() end})
print(type(gc))
print(rawequal(t, t), rawequal(t, {}), rawequal("a", "a"), rawequal(nil, nil))
local called = 0
local withcall = setmetatable({}, {__index = function() called = called + 1 end})
local _ = withcall.a, withcall.b
print(called)
local one = setmetatable({}, {__index = {x = "from mt"}})
print(one.x, rawget(one, "x"), getmetatable(one).__index.x)
local reused = {__index = function(_, k) return k end}
local r1, r2 = setmetatable({}, reused), setmetatable({}, reused)
print(r1.a, r2.b, getmetatable(r1) == getmetatable(r2))
reused.__index = function() return "changed" end
print(r1.a)
reused.__index = nil
print(r1.a)
print(#setmetatable({1, 2, 3}, {}), next(setmetatable({}, {})))
local inh = setmetatable({}, {__index = string})
print(inh.format("%d", 3), inh.nope)
local count = 0
local lazy = setmetatable({}, {__index = function(t, k) count = count + 1 local v = k .. k rawset(t, k, v) return v end})
print(lazy.ab, lazy.ab, lazy.cd, count)
