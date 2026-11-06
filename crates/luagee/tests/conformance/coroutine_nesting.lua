-- Coroutines: yield from nested Lua calls, across pcall, inside metamethods and iterators, errors, wrap propagation.
local function deep_yield(n) if n == 0 then coroutine.yield("bottom") return "back" end return deep_yield(n - 1) end
local co = coroutine.create(function() local r = deep_yield(100) coroutine.yield(r) end)
print(coroutine.resume(co))
print(coroutine.resume(co))
local y = coroutine.wrap(function()
  local ok, err = pcall(function() coroutine.yield("inside pcall") error("after yield") end)
  coroutine.yield(ok)
  return "done"
end)
print(y(), y(), y())
local it = coroutine.wrap(function() for i = 1, 3 do coroutine.yield(i, i * i) end end)
for a, b in it do print(a, b) end
local sorted_co = coroutine.wrap(function()
  local t = {3, 1, 2}
  local ok = pcall(table.sort, t, function(a, b) coroutine.yield("in sort") return a < b end)
  return ok
end)
print(sorted_co())
local err_co = coroutine.create(function() error("boom") end)
local ok, msg = coroutine.resume(err_co)
print(ok, (tostring(msg):gsub("^.-:%d+: ", "")), coroutine.status(err_co))
print(coroutine.resume(err_co))
local tbl_err = coroutine.create(function() error({code = 7}) end)
local ok2, e2 = coroutine.resume(tbl_err)
print(ok2, type(e2), e2.code)
local wrapped = coroutine.wrap(function() error("wrapped boom") end)
local ok3, e3 = pcall(wrapped)
print(ok3, (tostring(e3):gsub("^.-:%d+: ", "")))
local wrapped_tbl = coroutine.wrap(function() error({code = 9}) end)
local ok4, e4 = pcall(wrapped_tbl)
print(ok4, type(e4), type(e4) == "table" and e4.code)
local level = coroutine.wrap(function()
  local inner = coroutine.wrap(function() coroutine.yield("inner1") return "inner done" end)
  coroutine.yield(inner())
  coroutine.yield(inner())
  return "outer done"
end)
print(level(), level(), level())
local function nest(n)
  if n == 0 then return "core" end
  return coroutine.wrap(function() return nest(n - 1) end)()
end
print(nest(100))
local function chain(n)
  return coroutine.wrap(function()
    if n == 0 then coroutine.yield("leaf") return end
    local child = chain(n - 1)
    coroutine.yield(child() .. n)
  end)
end
print(chain(20)())
local passing = coroutine.create(function(a)
  local b = coroutine.yield(a)
  local c = coroutine.yield(b)
  return a, b, c
end)
print(coroutine.resume(passing, 1))
print(coroutine.resume(passing, 2))
print(coroutine.resume(passing, 3))
local count = 0
local cos = {}
for i = 1, 1000 do cos[i] = coroutine.create(function() coroutine.yield(i) return i * 2 end) end
for i = 1, 1000 do local _, v = coroutine.resume(cos[i]) count = count + v end
for i = 1, 1000 do local _, v = coroutine.resume(cos[i]) count = count + v end
print(count)
local died = coroutine.create(function() local x = nil; return x.field end)
local okd = coroutine.resume(died)
print(okd, coroutine.status(died))
local yielder = coroutine.wrap(function() return pcall(coroutine.yield, "yielded through pcall") end)
print(yielder())
print(yielder("resumed value"))
local co_in_co = coroutine.wrap(function()
  local inner = coroutine.create(function() coroutine.yield("i1") end)
  local _, v = coroutine.resume(inner)
  coroutine.yield("outer saw " .. v .. " " .. coroutine.status(inner))
end)
print(co_in_co())
print(select("#", coroutine.resume(coroutine.create(function() return 1, 2, 3 end))))
