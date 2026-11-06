-- Scoping: block locals, shadowing, repeat-until scope, local function recursion, globals vs locals, upvalue limits.
local x = "outer"
do local x = "inner" print(x) end
print(x)
do local x = x .. "+" print(x) end
local function shadow(x) local x = x or "default" return x end
print(shadow(), shadow("given"))
if true then local y = 1 end
print(y)
g_value = "global"
local function reads_global() return g_value end
local g_value = "local after"
print(reads_global(), g_value)
local f
do local hidden = 0 f = function() hidden = hidden + 1 return hidden end end
print(f(), f())
local r = 0
repeat local stop = r >= 2 r = r + 1 until stop
print(r)
local function even(n) if n == 0 then return true end return not even(n - 1) end
print(even(10), even(7))
local a = 1
local function a_reader() return a end
a = 2
print(a_reader())
local t = {}
for i = 1, 3 do local i = i * 2 t[#t + 1] = i end
print(table.concat(t, ","))
local n = 0
for i = 1, 3 do for i = 1, 3 do n = n + i end end
print(n)
local function many_locals()
  local a1, a2, a3, a4, a5, a6, a7, a8, a9, a10 = 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
  local b1, b2, b3, b4, b5, b6, b7, b8, b9, b10 = 11, 12, 13, 14, 15, 16, 17, 18, 19, 20
  return a1 + a10 + b1 + b10
end
print(many_locals())
local src = {}
for i = 1, 150 do src[#src + 1] = "local v" .. i .. " = " .. i end
src[#src + 1] = "return v1 + v150"
print(loadstring(table.concat(src, "\n"))())
src = {}
for i = 1, 250 do src[#src + 1] = "local v" .. i .. " = " .. i end
print(loadstring(table.concat(src, "\n")) == nil)
local ups = {}
for i = 1, 60 do ups[#ups + 1] = "local u" .. i .. " = " .. i end
local body = {}
for i = 1, 60 do body[#body + 1] = "u" .. i end
print(loadstring(table.concat(ups, "\n") .. "\nreturn function() return " .. table.concat(body, " + ") .. " end")()())
print(select("#", loadstring("return " .. ("1,"):rep(200) .. "1")()))
local function nest(n) local s = "return " .. ("("):rep(n) .. "1" .. (")"):rep(n) return loadstring(s) end
print(nest(50)(), nest(100) ~= nil)
local function block_depth(n) return loadstring(("do "):rep(n) .. ("end "):rep(n)) end
print(block_depth(50) ~= nil)
print(type(_G.x), type(_G.g_value), _G.g_value)
