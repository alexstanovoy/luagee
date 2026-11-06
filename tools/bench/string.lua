-- String formatting, substitution, searching and captures.
local n = 0
local parts = {}
for i = 1, 200000 do
  local name = string.format("record_%s_%d", (i % 2 == 0) and "alpha" or "beta", i % 100)
  local short = string.gsub(name, "record_", "")
  if string.find(short, "^alp") then n = n + 1 end
  local a, b = string.match(name, "(%a+)_(%d+)$")
  n = n + #a + tonumber(b)
  if i % 1000 == 0 then parts[#parts + 1] = short end
end
print(n, #table.concat(parts, ","))
