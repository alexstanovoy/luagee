-- Array and table-keyed hash parts.
local t = {}
for i = 1, 2000000 do t[i] = i * 2 end
local s = 0
for r = 1, 5 do for i = 1, #t do s = s + t[i] end end
local h = {}
local keys = {}
for i = 1, 1000 do keys[i] = {} end
for r = 1, 300 do for i = 1, 1000 do local k = keys[i]; h[k] = (h[k] or 0) + i end end
local c = 0
for k, v in pairs(h) do c = c + v end
print(s, c)
