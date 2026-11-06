-- module(), package.seeall, package.loaded, and calling module functions from outside.
local print, type, setfenv, pcall, package, G = print, type, setfenv, pcall, package, _G
module("mymod", package.seeall)
function hello() return "hello from " .. _NAME end
counter = 0
function bump() counter = counter + 1 return counter end
local function hidden() return "hidden" end
function reveal() return hidden() end
print(_NAME, _PACKAGE, _M == mymod)
local self = _M
module("other")
function add(a, b) return a + b end
print(_NAME, type(_M))
local o = _M
setfenv(1, G)
print(type(mymod), mymod.hello(), mymod.bump(), mymod.bump(), mymod.counter, mymod.reveal())
print(mymod.hidden, mymod._NAME, mymod._M == mymod, mymod._PACKAGE)
print(other.add(2, 3), other == o, package.loaded.mymod == mymod, package.loaded["other"] == other)
print(counter, hello)
module("deep.nested.name", package.seeall)
function f() return "deep" end
setfenv(1, G)
print(deep.nested.name.f(), deep.nested.name._NAME, deep.nested.name._PACKAGE)
module("plain")
x = 1
local ok = pcall(function() return type(print) end)
setfenv(1, G)
print(plain.x, ok, plain.print)
print(pcall(module) == false)
local m2 = {}
package.loaded.preset = m2
module("preset")
setfenv(1, G)
print(package.loaded.preset == m2, preset)
