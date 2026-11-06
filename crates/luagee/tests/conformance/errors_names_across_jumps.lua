-- Error messages name the variable a value came from even across the jumps of `and`, `or` and `if`, as LuaJIT's
-- backward scan of the bytecode finds it.
local function message(source)
  local ok, err = pcall(loadstring(source, "=s"))
  return err
end
print(message([[aaa=9
repeat until 3==3
local x=math.sin(math.cos(3))
if math.sin(1) == x then return math.sin(1) end
local a,b = 1, {
  {x='a'..'b'..'c', y='b', z=x},
  {1,2,3,4,5} or 3+3<=3+3,
  3+1>3+1,
  {d = x and aaa[x or y]}}
]]))
print(message("aaa={}; (aaa or aaa)()"))
print(message("aaa={}; x=(aaa or aaa)+(aaa and aaa)"))
print(message("local t = {} if t then t = t.missing end t.field = 1"))
print(message("local s = nil; local u = s and 1 or s; u:method()"))
print(message("local a = {} local b = a.x.y"))
print(message("x = nil; local y = x; y()"))
print(message("local up; (function() local z = up; z.k = 1 end)()"))
print(message("bbb = 1; local c = bbb + {}"))
print(message("local f = function() end; f()()"))
