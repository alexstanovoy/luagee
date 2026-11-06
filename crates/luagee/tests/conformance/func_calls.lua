-- Functions: multiple results in all positions, method calls, definitions forms, recursion, deep tail calls.
local function mr() return 1, 2, 3 end
print(mr())
print(mr(), 10)
print(10, mr())
print((mr()))
local t = {mr(), mr()}
print(#t)
t = {mr(), (mr())}
print(#t)
t = {x = mr(), mr()}
print(t.x, #t)
print(#{[1] = mr()}, ({[1] = mr()})[1])
print(type(mr), type(print), type(nil), type(2), type("x"), type({}), type(true), type(coroutine.create(mr)))
local function noret() end
local function emptyret() return end
print(noret(), emptyret())
print((noret()), (emptyret()))
print(select("#", noret()), select("#", emptyret()), select("#", (noret())))
print((pcall(tostring)))
local obj = {name = "obj", value = 5}
function obj:get() return self.value end
function obj.static(x) return x end
function obj:add(n) self.value = self.value + n return self end
print(obj:get(), obj.get(obj), obj.static(3), obj:add(2):add(3):get())
local nested = {a = {b = {}}}
function nested.a.b:c(x) return self == nested.a.b, x end
function nested.a.f(x) return x end
print(nested.a.b:c(7), nested.a.f(8))
print(("x"):rep(3), ("%d-%d"):format(1, 2), #("abc"):upper())
local s = "hello"
print(s:sub(2, 3), s:len(), s:upper():lower())
print(type(string.len), string.len("abc"))
local f = function(...) return ... end
print(f(1, 2))
local function fact(n) if n <= 1 then return 1 end return n * fact(n - 1) end
print(fact(10), fact(20), fact(25))
local function fib(n) if n < 2 then return n end return fib(n - 1) + fib(n - 2) end
print(fib(20))
local even, odd
function even(n) if n == 0 then return true end return odd(n - 1) end
function odd(n) if n == 0 then return false end return even(n - 1) end
print(even(100000), odd(100000), even(7))
local function loop(n, acc) if n == 0 then return acc end return loop(n - 1, acc + 1) end
print(loop(100000, 0))
local function tail_pcall(n) if n == 0 then return "done" end return (select(2, pcall(tail_pcall, n - 1))) end
print(tail_pcall(50))
local function deep(n) if n == 0 then return 0 end return 1 + deep(n - 1) end
print(deep(1000), deep(5000))
local function tail_vararg(n, ...) if n == 0 then return select("#", ...) end return tail_vararg(n - 1, ...) end
print(tail_vararg(100000, 1, 2, 3))
local mt = setmetatable({}, {__call = function(self, n) if n == 0 then return "called" end return self(n - 1) end})
print(mt(50))
local function ret_call_method(o, n) if n == 0 then return o.value end return o:next(n) end
local o2 = {value = "end"}
function o2:next(n) return ret_call_method(self, n - 1) end
print(ret_call_method(o2, 50000))
local function id(...) return ... end
print(id(id(id(1, 2, 3))))
print(#tostring(print) > 0, tostring(print) == tostring(print), print == print, mr == mr, function() end == function() end)
local function args_eval_order()
  local log = {}
  local function mark(x) log[#log + 1] = x return x end
  local function three(a, b, c) return a + b + c end
  three(mark(1), mark(2), mark(3))
  return table.concat(log, ",")
end
print(args_eval_order())
print(pcall(function() local a; a() end) == false, pcall(function() local a = {}; a.b.c = 1 end) == false)
print(pcall(function() return ("x")() end) == false, pcall(function() return (1)() end) == false)
print(string.rep("ab", 3, "-"), (string.gsub("abc", "b", "X")))
print(type(_G), _G == _G._G, _G.print == print, _VERSION)
