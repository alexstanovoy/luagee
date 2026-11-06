-- Program: module pattern with environments, memoize/curry/compose, event emitter, iterators and state machines.
local function memoize(f) local cache = {} return function(x) local v = cache[x] if v == nil then v = f(x) cache[x] = v end return v end end
local calls = 0
local slow = memoize(function(x) calls = calls + 1 return x * x end)
print(slow(4), slow(4), slow(5), calls)
local function curry(f, a) return function(...) return f(a, ...) end end
local function compose(f, g) return function(...) return f(g(...)) end end
local add = function(a, b) return a + b end
print(curry(add, 10)(5), compose(curry(add, 1), curry(add, 2))(3))
local Emitter = {}
Emitter.__index = Emitter
function Emitter.new() return setmetatable({handlers = {}}, Emitter) end
function Emitter:on(name, f) local list = self.handlers[name] or {} self.handlers[name] = list list[#list + 1] = f return self end
function Emitter:emit(name, ...) local n = 0 for _, f in ipairs(self.handlers[name] or {}) do f(...) n = n + 1 end return n end
local log = {}
local em = Emitter.new():on("hit", function(d) log[#log + 1] = "a" .. d end):on("hit", function(d) log[#log + 1] = "b" .. d end)
print(em:emit("hit", 5), em:emit("miss"), table.concat(log, ","))
local function range(a, b, step)
  step = step or 1
  return coroutine.wrap(function() for i = a, b, step do coroutine.yield(i) end end)
end
local acc = {}
for i in range(1, 10, 3) do acc[#acc + 1] = i end
print(table.concat(acc, ","))
local function map(f, t) local r = {} for i, v in ipairs(t) do r[i] = f(v) end return r end
local function filter(p, t) local r = {} for _, v in ipairs(t) do if p(v) then r[#r + 1] = v end end return r end
local function reduce(f, init, t) local a = init for _, v in ipairs(t) do a = f(a, v) end return a end
local nums = {1, 2, 3, 4, 5, 6}
print(reduce(add, 0, map(function(x) return x * x end, filter(function(x) return x % 2 == 0 end, nums))))
local machine = {state = "idle", log = {}}
local transitions = {idle = {start = "running"}, running = {pause = "paused", stop = "idle"}, paused = {resume = "running", stop = "idle"}}
function machine:fire(ev)
  local nxt = transitions[self.state][ev]
  if not nxt then self.log[#self.log + 1] = "!" .. ev return false end
  self.log[#self.log + 1] = self.state .. ">" .. nxt
  self.state = nxt
  return true
end
for _, ev in ipairs({"start", "pause", "start", "resume", "stop"}) do machine:fire(ev) end
print(machine.state, table.concat(machine.log, " "))
local sandbox = setmetatable({}, {__index = {print = print, tostring = tostring}})
local chunk = loadstring("counter = (counter or 0) + 1 local r = tostring(counter) return r, os")
setfenv(chunk, sandbox)
print(chunk()) print(chunk())
print(sandbox.counter, rawget(_G, "counter"))
local obj = setmetatable({}, {__index = function(t, k) return function(self, ...) return k .. ":" .. select("#", ...) end end})
print(obj:anything(1, 2), obj:other(), obj.x(nil, nil, nil))
local lazy_calls = 0
local lazy = setmetatable({}, {__index = function(t, k) lazy_calls = lazy_calls + 1 local v = k * 2 rawset(t, k, v) return v end})
print(lazy[1], lazy[1], lazy[2], lazy_calls)
local function varsum(...) local s = 0 for _, v in ipairs({...}) do s = s + v end return s, select("#", ...) end
print(varsum(), varsum(1), varsum(1, 2, 3))
local pipeline = {function(x) return x + 1 end, function(x) return x * 2 end, function(x) return x - 3 end}
local value = 5
for _, f in ipairs(pipeline) do value = f(value) end
print(value)
