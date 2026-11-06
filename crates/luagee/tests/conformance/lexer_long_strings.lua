-- Long strings and comments: levels, first newline skipped, no escapes, nesting of other levels.
local function bytes(s)
  local t = {}
  for i = 1, #s do t[i] = s:byte(i) end
  return table.concat(t, ",")
end
print([[hello]], [==[a]]b]==], [=[x]==]=])
print(bytes([[
first newline skipped]]))
print(bytes([[

two newlines]]))
print(bytes([==[
]==]))
print(bytes([[a\nb\z]]))
print(bytes([[a
b]]))
print(#[[]], #[==[]==])
print([[ [[ ]], [=[ ]] ]=])
--[[ comment
spanning lines ]] print("after block comment")
--[==[ comment with ]] inside
]==] print("after level-2 comment")
--[ not a long comment
print("line comment variant")
--[=[ unfinished level mismatch ]] ]=] print("mismatch ok")
---[[ this is a line comment, so the next line runs
print("triple dash")
--]]
print(loadstring("return [[unfinished") == nil)
print(loadstring("--[[ unfinished") == nil)
print(loadstring("return [=[x]]") == nil)
print(loadstring("return [=x]]") == nil)
print(bytes([[a
]]))
print(bytes("a\r\nb") , bytes([[a
b]]))
local f = loadstring("return [[a\r\nb]]")
print(bytes(f()))
f = loadstring("return [[a\n\rb]]")
print(bytes(f()))
f = loadstring("return [[\r\nx]]")
print(bytes(f()))
