-- Short string escapes (\n \t \\ \" \' \ddd \xXX \z, line continuation) and the strings' bytes.
local function bytes(s)
  local t = {}
  for i = 1, #s do t[i] = s:byte(i) end
  return table.concat(t, ",")
end
print(bytes("\a\b\f\n\r\t\v\\\"\'"))
print(bytes('\a\b\f\n\r\t\v\\\"\''))
print(bytes("\65\066\0067"), bytes("\0"), bytes("a\0b"), #"a\0b")
print(bytes("\255"), bytes("\1234"))
print(bytes("\x41\x4a\xFF\x00"))
print(bytes("a\z
             b"))
print(bytes("a\z   b\z
   c"))
print(bytes("line\
continued"))
print(bytes("x\
"))
print(bytes("\
"))
print(bytes("tab\there"))
print(loadstring('return "\\q"') == nil)
print(loadstring('return "\\256"') == nil)
print(loadstring('return "\\xZZ"') == nil)
print(loadstring('return "unfinished') == nil)
print(loadstring('return "new\nline"') == nil)
print(loadstring('return "a\\') == nil)
print(bytes("\u{48}") )
print(#"\x00\x01", ("\x41"):rep(3))
print("ünï", #"ünï", bytes("é"))
print('single "double" inside', "double 'single' inside")
print(("%q"):format("a\nb\0c\"d\\"))
