-- expect: error error object is a table value
error(setmetatable({}, {__tostring = function() while true do end end}))
