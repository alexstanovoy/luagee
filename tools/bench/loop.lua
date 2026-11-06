-- Arithmetic and branches in a numeric loop.
local s = 0
for i = 1, 30000000 do
  local x = i % 7
  if x < 3 then s = s + x * 2 else s = s - 1 end
end
print(s)
