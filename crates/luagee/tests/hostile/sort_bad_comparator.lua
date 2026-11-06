-- expect: error order function
local t = {} for i = 1, 1000 do t[i] = i % 7 end table.sort(t, function(a, b) return true end) return "sorted"
