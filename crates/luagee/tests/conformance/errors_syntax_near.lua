-- Syntax errors as LuaJIT words them: the token near the error as it was written, and what an assignment wants.
for _, s in ipairs{
  "syntax error",
  "1.000",
  "x = 1 1.5e3",
  "x = 1 0x10",
  "[[a]]",
  "'aa'",
  "x = 1 'a\\tb'",
  "x = 1 \"q\"",
  "x = 1 [==[long]==]",
  "x = 1 " .. string.rep("n", 80),
  "x = 1 '" .. string.rep("s", 50) .. "'",
  "a.b c",
  "a[1] c",
  "a, b c",
  "(a) = 1",
  "a, (b) = 1",
  "(a), b = 1",
  "f() = 1",
  "f(), x = 1",
  "a.b:c() d",
  "x = }",
  "local x = 1 local",
  "local function 1",
  "for 1",
} do
  local f, message = loadstring(s, "=s")
  print(f and "compiles" or message)
end
