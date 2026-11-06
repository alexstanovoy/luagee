-- expect: error loop
local t = {} for i = 1, 1e4 do t = setmetatable({}, {__index = t}) end return t.x
