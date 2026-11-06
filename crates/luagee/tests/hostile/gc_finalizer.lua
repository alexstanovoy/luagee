-- expect: error nil value
local p = newproxy(true) getmetatable(p).__gc = function() while true do end end p = nil collectgarbage() return "finalized"
