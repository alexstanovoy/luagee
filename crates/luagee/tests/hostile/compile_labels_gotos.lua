-- expect: ok true
local parts = {} for i = 1, 3e4 do parts[#parts + 1] = "::l" .. i .. ":: " end
for i = 1, 3e4 do parts[#parts + 1] = "goto l" .. i .. " " end
print(loadstring(table.concat(parts)) ~= nil)
