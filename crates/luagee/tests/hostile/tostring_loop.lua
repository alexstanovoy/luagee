-- expect: limit fuel_per_call
return tostring(setmetatable({}, {__tostring = function() while true do end end}))
