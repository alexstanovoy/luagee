-- expect: limit memory
local i = 0 while true do i = i + 1 rawset(_G, "k" .. i, i) end
