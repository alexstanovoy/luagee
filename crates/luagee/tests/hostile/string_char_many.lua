-- expect: error too many
local t = {} for i = 1, 1e5 do t[i] = 65 end return #string.char(unpack(t))
