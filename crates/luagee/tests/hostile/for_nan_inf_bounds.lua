-- expect: ok 21
local n = 0 for i = 1, 0/0 do n = n + 1 end for i = 0/0, 1 do n = n + 1 end for i = 1, -math.huge do n = n + 1 end for i = 1, math.huge do n = n + 1 if n > 10 then break end end for i = -math.huge, 1 do n = n + 1 if n > 20 then break end end print(n)
