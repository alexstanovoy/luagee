-- Chunk names as error messages show them, shortened as LuaJIT shortens them.
local function shown(source, name)
  local f, message = loadstring(source, name)
  return message
end
local long = string.rep("x", 70)
print(shown("x = = 1"))
print(shown("local a; a = {{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{{ ="))
print(shown(string.rep("a", 47) .. " = ="))
print(shown(string.rep("a", 48) .. " = ="))
print(shown(string.rep("a", 40) .. "\n = ="))
print(shown("\tx = ="))
print(shown("x = =", "=" .. long))
print(shown("x = =", "=short"))
print(shown("x = =", "@" .. long))
print(shown("x = =", "@" .. string.rep("y", 59)))
print(shown("x = =", "@" .. string.rep("y", 60)))
print(shown("x = =", "plain name"))
print(shown("x = =", ""))
