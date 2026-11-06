-- `return` and `break` end their block, as in Lua 5.1 and LuaJIT: what follows them must close it, and the message
-- names what closes it. Semicolons may follow them.
for _, s in ipairs{
  "break label",
  "while true do break print(1) end",
  "while true do break ::x:: end",
  "while true do break;; end",
  "if x then break x() else end",
  "for i = 1, 2 do break; ; end",
  "while true do return 1 print(1) end",
  "if x then return 1 print(1) end",
  "repeat return 1 x() until y",
  "function f() return 1 x() end",
  "return 1 x()",
  "return;;",
  "do return end print(1)",
} do
  local f, message = loadstring(s)
  print(f and "compiles" or message)
end
