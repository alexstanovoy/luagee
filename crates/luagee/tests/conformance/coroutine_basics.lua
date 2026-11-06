-- Coroutines: create/resume/yield/status/wrap/running, values in both directions, dead coroutines.
local co = coroutine.create(function(a, b)
  print("start", a, b)
  local c = coroutine.yield(a + b)
  print("got", c)
  local d, e = coroutine.yield(c * 2)
  print("got", d, e)
  return "finished", 99
end)
print(coroutine.status(co))
print(coroutine.resume(co, 1, 2))
print(coroutine.status(co))
print(coroutine.resume(co, 10))
print(coroutine.resume(co, "x", "y"))
print(coroutine.status(co))
print(coroutine.resume(co))
print(coroutine.resume(co))
print(coroutine.status(co))
local gen = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i) end end)
print(gen(), gen(), gen(), gen())
print(pcall(gen) == false)
local function producer()
  return coroutine.create(function()
    for i = 1, 3 do coroutine.yield("item" .. i) end
    return "end"
  end)
end
local p = producer()
while true do
  local ok, v = coroutine.resume(p)
  print(ok, v, coroutine.status(p))
  if coroutine.status(p) == "dead" then break end
end
local main, is_main = coroutine.running()
print(type(main), is_main)
local inside
local c2 = coroutine.create(function()
  local me, ismain = coroutine.running()
  inside = me
  print(type(me), ismain, coroutine.status(me))
end)
coroutine.resume(c2)
print(inside == c2, coroutine.status(c2))
local statuses = {}
local c3
c3 = coroutine.create(function()
  statuses[#statuses + 1] = coroutine.status(c3)
  local c4 = coroutine.create(function() statuses[#statuses + 1] = coroutine.status(c3) end)
  coroutine.resume(c4)
  coroutine.yield()
end)
coroutine.resume(c3)
statuses[#statuses + 1] = coroutine.status(c3)
print(table.concat(statuses, ","))
print(coroutine.resume(coroutine.create(function(...) return select("#", ...), ... end), nil, nil))
print(coroutine.resume(coroutine.create(function() return end)))
print(coroutine.resume(coroutine.create(function() coroutine.yield() end)))
print(coroutine.resume(coroutine.create(function() coroutine.yield(1, nil, 3) end)))
print(pcall(coroutine.resume, nil) == false, pcall(coroutine.create, 1) == false, pcall(coroutine.status, {}) == false)
print(select("#", coroutine.resume(coroutine.create(function() return nil end))))
local wrapped = coroutine.wrap(function(...) local args = {...} coroutine.yield(#args) return "bye" end)
print(wrapped(1, 2, 3), wrapped(), pcall(wrapped) == false)
local self_co
self_co = coroutine.create(function() return coroutine.resume(self_co) end)
print(coroutine.resume(self_co))
print(pcall(coroutine.yield, 1) == false)
local acc = {}
local sum_co = coroutine.wrap(function(first)
  local total = first
  while true do
    local n = coroutine.yield(total)
    if not n then return total end
    total = total + n
  end
end)
print(sum_co(1), sum_co(2), sum_co(3), sum_co(nil))
