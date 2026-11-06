-- goto and labels with LuaJIT's rules: continue idiom, backward jumps, nested loops, scope errors.
local acc
for i = 1, 5 do
  if i % 2 == 0 then goto continue end
  acc = (acc or "") .. i
  ::continue::
end
print(acc)
acc = nil
local n = 0
::top::
n = n + 1
if n < 5 then goto top end
print(n)
for i = 1, 3 do
  for j = 1, 3 do
    if j == 2 then goto next_i end
    acc = (acc or "") .. i .. j .. " "
  end
  ::next_i::
end
print(acc)
acc = nil
do
  goto skip
  print("skipped")
  ::skip::
end
local function find(t, x)
  for i, v in ipairs(t) do
    if v == x then goto found end
  end
  do return nil end
  ::found::
  return true
end
print(find({1, 2, 3}, 2), find({1, 2, 3}, 5))
local function compiles(src) return loadstring(src) ~= nil end
print(compiles("goto l1; local a; ::l1:: print(a)"))
print(compiles("goto l1; local a; ::l1::"))
print(compiles("do goto l1; local a; ::l1:: end"))
print(compiles("goto nowhere"))
print(compiles("::a:: ::a::"))
print(compiles("::a:: do ::a:: end"))
print(compiles("do ::a:: end ::a::"))
print(compiles("goto a; do ::a:: end"))
print(compiles("do goto a end ::a::"))
print(compiles("repeat goto cont; local z = 1; ::cont:: until z"))
print(compiles("repeat local z = 1; goto cont; ::cont:: until z"))
print(compiles("while true do goto out end ::out::"))
print(compiles("local function f() goto out end ::out::"))
print(compiles("::l:: goto l"))
print(compiles("goto = 1"))
print(compiles("local goto"))
print(compiles("x.goto = 1"))
print(compiles("return function() ::a:: end"))
local k = 0
do
  ::again::
  k = k + 1
  if k < 3 then goto again end
end
print(k)
local closures = {}
do
  local i = 1
  ::loop::
  local captured = i
  closures[i] = function() return captured end
  i = i + 1
  if i <= 3 then goto loop end
end
print(closures[1](), closures[2](), closures[3]())
