-- expect: ok false	true	table
local object = setmetatable({}, {__tostring = function() return {} end}) local ok = pcall(print, object) local ok2, r = pcall(tostring, object) print(ok, ok2, type(r))
