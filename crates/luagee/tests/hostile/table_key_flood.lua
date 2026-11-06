-- expect: limit memory
local t = {} local i = 0 while true do i = i + 1 t["key" .. i] = i end
