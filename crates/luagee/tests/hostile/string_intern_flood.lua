-- expect: limit memory
local t = {} for i = 1, 1e9 do t[i] = tostring(i) .. "suffix" end
