-- expect: ok done
local t = {} for i = 1, 100 do t[i] = i end pcall(table.sort, t, function(a, b) return (a * 7 + b * 3) % 5 < 2 end) print("done")
