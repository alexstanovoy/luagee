-- expect: limit memory
local t = {} for i = 1, 1e8 do t[i] = coroutine.create(function() end) end
