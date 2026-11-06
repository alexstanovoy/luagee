-- __index and __newindex with tables and functions, chains, rawget/rawset, inheritance, shadowing.
local base = {a = 1, shared = "base"}
local derived = setmetatable({b = 2, shared = "derived"}, {__index = base})
print(derived.a, derived.b, derived.shared, derived.missing, rawget(derived, "a"))
local c = setmetatable({}, {__index = derived})
print(c.a, c.b, c.shared, c.zzz)
local calls = {}
local f = setmetatable({}, {__index = function(t, k) calls[#calls + 1] = tostring(k) return k .. "!" end})
print(f.x, f[1], f.x, rawget(f, "x"), #calls)
f.x = "set"
print(f.x, #calls)
local log = {}
local w = setmetatable({}, {__newindex = function(t, k, v) log[#log + 1] = k .. "=" .. tostring(v) rawset(t, k, v) end})
w.a = 1
w.a = 2
w.b = nil
print(table.concat(log, ","), w.a)
local store = {}
local proxy = setmetatable({}, {__newindex = store, __index = store})
proxy.x = 5
print(rawget(proxy, "x"), store.x, proxy.x)
local chain = setmetatable({}, {__newindex = setmetatable({}, {__newindex = function(t, k, v) log[#log + 1] = "deep:" .. k end})})
chain.q = 1
print(log[#log], rawget(chain, "q"))
local ro = setmetatable({}, {__newindex = function() error("read-only") end, __index = {v = 1}})
print(pcall(function() ro.v = 2 end), ro.v)
local depth = {}
local cur = depth
for i = 1, 50 do cur = setmetatable({}, {__index = cur}) end
depth.deep = "found"
print(cur.deep, cur.nope)
local counter = setmetatable({}, {__index = function(t, k) local v = 0 rawset(t, k, v) return v end})
counter.a = counter.a + 1
counter.a = counter.a + 1
print(counter.a, counter.b)
local arr = setmetatable({}, {__index = function(t, i) return i * i end})
print(arr[3], arr[10], arr[-2], rawget(arr, 3))
local mt = {__index = function(t, k) return "idx" end}
local nilfield = setmetatable({present = false}, mt)
print(nilfield.present, nilfield.absent)
local both = setmetatable({}, {__index = function() return "i" end, __newindex = function(t, k, v) rawset(t, k, "n" .. v) end})
both.k = "v"
print(both.k, both.other)
print(rawget(_G, "print") == print, rawset({}, 1, 2)[1], rawequal("a", "a"), rawequal({}, {}), rawequal(1, 1.0))
print(pcall(rawset, 1, 2, 3) == false, pcall(rawget, "x", 1) == false)
local rn = pcall(rawset, {}, nil, 1)
print(rn)
local nan_key = pcall(rawset, {}, 0/0, 1)
print(nan_key)
local ok1 = pcall(function() local t = {} t[nil] = 1 end)
local ok2 = pcall(function() local t = {} t[0/0] = 1 end)
print(ok1, ok2, ({})[nil], ({})[0/0])
local keys = setmetatable({}, {__index = function(t, k) return type(k) end})
print(keys[1], keys["s"], keys[true], keys[print], keys[keys])
