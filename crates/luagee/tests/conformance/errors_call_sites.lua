-- Error messages name a function the way its caller referred to it, and a function called in tail position reports
-- from its caller's caller (its own frame is gone), as LuaJIT does.
local function try(f, ...) print(pcall(f, ...)) end

-- error() with a number adds a position, as with a string.
try(function() error(5) end)
try(function() error(5.5, 1) end)
try(function() error(true) end)
try(function() assert(false, 5) end)
try(function() return error(5) end)

-- Tail calls: the frame that made the call is gone.
local function f() return error("boom") end
try(f)
local function g() f() end
try(g)
local function h() return error("lvl2", 2) end
local function k() h() end
try(k)
local tc = table.concat
local function w(t) return tc(t) end
local function u() w(1) end
try(u)
try(function() return string.rep() end)
try(function() local c = coroutine.wrap(function() return string.rep() end) c() end)

-- Names from the call site.
try(function() local x = tc(1) end)
local s = string
try(function() local x = s.rep() end)
try(function() local x = ("x"):rep() end)
try(function() local x = ("x"):rep("a") end)
try(function() local x = s.rep(nil, 1) end)
try(function() local t = {f = string.rep} local x = t.f() end)
try(function() local x = (string.rep)() end)
try(function() local x = select(2, 1, string.rep)() end)
try(function() local t = setmetatable({}, {__index = string.rep}) local x = t[{}] end)
try(function() local t = setmetatable({}, {__add = string.rep}) local x = t + 1 end)
try(function() local name = "rep" local x = string[name]() end)
try(function() local x = rep() end)
try(table.sort, 1)
try(function() local t = {3, 1, 2} table.sort(t, math.max) end)

-- Upvalues and operands named after a call or a closure in between.
local v2, v4 = "s", {}
try(function() local q = v4 .. "a" end)
try(function() local q = "a" .. v4 end)
try(function() local q = "a" .. v4 .. "b" end)
try(function() v2[(function() return 1 end)] = 1 end)
try(function() local q = v2[math.floor(1)] end)
try(function() local q = v4 * (function() return 1 end) end)

-- Arguments that are missing.
try(function() local x = string.sub("abc") end)
try(function() local x = string.len() end)
try(function() local x = table.remove() end)
try(function() local x = table.getn() end)
try(function() local x = pairs() end)
try(function() local x = ipairs() end)

-- error(message, 2) in a metamethod points at the instruction that set it off.
local strict = setmetatable({}, {__index = function(t, k) error("no field " .. k, 2) end})
try(function()
  local a = 1
  return strict.x
end)
try(function()
  local y = strict.y
  return y
end)
local object = setmetatable({}, {__add = function(a, b) error("bad add", 2) end, __index = function(t, k) return string.rep(k) end})
try(function() local z = object + 1 end)
try(function() local z = object[{}] end)
local function check(x) if type(x) ~= "number" then error("expected number", 2) end end
try(function() check("a") end)

-- A wrapped coroutine's error gets the position of the wrapper's caller.
local c = coroutine.wrap(function() error("boom") end)
try(c)
try(function() local d = coroutine.wrap(function() error("y", 0) end) d() end)
try(function() local d = coroutine.wrap(function() local x = nil + 1 end) d() end)

-- Names found past jumps and moves, as LuaJIT finds them.
local v1, v5 = 1, "x"
local function f1() return 1 end
try(function() v2[(211 or f1())] = v2[v1] end)
try(function() v1[-1] = v5[string.upper((-1 == -16))] end)
try(function() local x = (nil .. (v1 * 2^53)) end)
try(function() local x = math.ceil("%d", ("1e2" or v2)) end)
try(function() local a = v2 local b = a.x.y end)
try(function() for i = 1, 2 do local q = v2.k + i end end)

-- tonumber asks for its string before it checks the base.
try(function() local x = tonumber(nil, 37) end)
try(function() local x = tonumber(v2.k0, v4) end)
