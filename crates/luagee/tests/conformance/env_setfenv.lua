-- Function environments: setfenv/getfenv, inheritance by nested functions, module(), globals via _G and _ENV absence.
local function show_env(f) return getfenv(f) end
print(getfenv(0) == _G, getfenv(1) == _G, getfenv(print) == _G, getfenv() == _G)
x = "global x"
local function read_x() return x end
print(read_x())
local env = {x = "env x"}
print(setfenv(read_x, env) == read_x, read_x(), getfenv(read_x) == env)
print(x)
local function writes() y = "written" end
local env2 = {}
setfenv(writes, env2)
writes()
print(env2.y, y)
local function maker()
  return function() return value end
end
local env3 = {value = "v3"}
setfenv(maker, env3)
local inner = maker()
print(inner(), getfenv(inner) == env3)
local function outer()
  local function in1() return value end
  return in1
end
print(outer()())
setfenv(outer, {value = "from outer env"})
print(outer()())
local f = loadstring("return a + b")
setfenv(f, {a = 1, b = 2})
print(f())
local g = loadstring("c = 5 return c")
local sandbox = {}
setfenv(g, sandbox)
print(g(), sandbox.c, c)
local h = loadstring("return print")
setfenv(h, {})
print(h())
setfenv(h, setmetatable({}, {__index = _G}))
print(h() == print)
local function lvl() local e = getfenv(2) return e end
local lenv = {}
local function caller() local e = lvl() return e end
setfenv(caller, lenv)
print(caller() == lenv)
local function set_level()
  setfenv(1, {marker = "level1"})
  return marker
end
print(set_level())
print(marker)
print((pcall(setfenv, print, {})))
print(pcall(setfenv, 1, 1) == false)
print(pcall(getfenv, 1000) == false)
print(pcall(setfenv, loadstring("return 1"), nil) == false)
local counter_env = setmetatable({}, {__index = _G, __newindex = function(t, k, v) rawset(t, k, v) end})
local code = loadstring("n = (n or 0) + 1 return n")
setfenv(code, counter_env)
print(code(), code(), counter_env.n, n)
local fenv_of_c = getfenv(coroutine.wrap(function() end))
print(fenv_of_c == _G)
local co_env = {tag = "co"}
local cof = function() return tag end
setfenv(cof, co_env)
print(coroutine.wrap(cof)())
local shared = {v = 1}
local s1 = loadstring("return v")
local s2 = loadstring("v = v + 1")
setfenv(s1, shared) setfenv(s2, shared)
s2() s2()
print(s1())
_G.viaG = "via _G"
print(viaG, _G["viaG"], rawget(_G, "viaG"))
print(_ENV == nil)
setmetatable(_G, {__index = function(_, k) return "undefined:" .. k end})
print(undefined_name)
setmetatable(_G, nil)
print(undefined_name)
