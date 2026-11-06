-- expect: error loop
local t = setmetatable({}, {}) getmetatable(t).__index = t return t.x
