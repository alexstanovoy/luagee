-- Generic for: pairs/ipairs/next, custom iterators, the three-value protocol, modification during traversal.
local acc
local function sorted_keys(t)
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = tostring(k) end
  table.sort(keys)
  return table.concat(keys, ",")
end
print(sorted_keys({10, 20, 30, a = 1, b = 2, [100] = 3, [1.5] = 4}))
local t = {"a", "b", "c", nil, "e"}
local n = 0
for i, v in ipairs(t) do n = n + 1 end
print(n)
for i, v in ipairs({}) do print("never") end
for i, v in ipairs({1, 2, 3}) do acc = (acc or 0) + i * v end
print(acc)
acc = nil
for k, v in next, {x = 1} do print(k, v) end
local function range(limit)
  return function(max, current)
    current = current + 1
    if current <= max then return current end
  end, limit, 0
end
local sum = 0
for i in range(5) do sum = sum + i end
print(sum)
local function iter_state(s, c) if c < s then return c + 1, c * c end end
for a, b in iter_state, 3, 0 do acc = (acc or "") .. a .. ":" .. b .. " " end
print(acc)
acc = nil
local calls = 0
local function counter() calls = calls + 1 if calls <= 3 then return calls end end
for v in counter do acc = (acc or 0) + v end
print(acc, calls)
acc = nil
for a, b, c, d in function(_, ctl) if not ctl then return 1, 2, 3, 4 end end do print(a, b, c, d) end
local seen = {}
local mt = {1, 2, 3, 4, 5}
for i, v in ipairs(mt) do seen[#seen + 1] = v if i == 2 then mt[4] = nil end end
print(#seen)
local clear = {a = 1, b = 2, c = 3, d = 4}
local count = 0
for k in pairs(clear) do clear[k] = nil count = count + 1 end
print(count, next(clear))
local fns = {}
for k, v in pairs({"x", "y"}) do fns[k] = function() return k, v end end
print(fns[1](), fns[2]())
print(pcall(function() for k in 5 do end end) == false)
print(pcall(function() for k in nil do end end) == false)
print(pcall(pairs) == false, pcall(ipairs) == false, pcall(pairs, nil) == false, pcall(ipairs, 5) == false)
for i, v in ipairs({n = 3, 7, 8}) do acc = (acc or 0) + v end
print(acc)
acc = nil
local idx = setmetatable({}, {__index = function(_, i) if i <= 3 then return i * 10 end end})
for i, v in ipairs(idx) do acc = (acc or 0) + v end
print(acc)
acc = nil
print(next({}), next({5}), type(next), select("#", next({})))
print(rawequal(pairs({}), next), type(select(2, pairs({}))), select(3, pairs({})))
local ip, st, ctl = ipairs({})
print(type(ip), type(st), ctl)
for k, v in pairs(setmetatable({}, {__pairs = function() error("pairs metamethod used") end})) do end
print("__pairs ignored")
