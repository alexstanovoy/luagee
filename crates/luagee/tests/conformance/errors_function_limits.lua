-- LuaJIT's limits on a function: 200 active locals and 60 upvalues, worded as LuaJIT words them.
local function locals(count, nested)
  local names = {}
  for i = 1, count do names[i] = "a" .. i end
  local body = "local " .. table.concat(names, ", ") .. " = 1\n"
  if nested then
    return "x = 1\nlocal function f()\n" .. body .. "end"
  end
  return body
end
print(loadstring(locals(200)) ~= nil)
print(select(2, loadstring(locals(201))))
print(loadstring(locals(200, true)) ~= nil)
print(select(2, loadstring(locals(201, true))))

local function upvalues(count)
  local names, uses = {}, {}
  for i = 1, count do
    names[i] = "u" .. i
    uses[i] = "u" .. i .. " = 2"
  end
  return "local " .. table.concat(names, ", ") .. "\nlocal function f()\n" .. table.concat(uses, "\n") .. "\nend"
end
print(loadstring(upvalues(60)) ~= nil)
print(select(2, loadstring(upvalues(61))))
