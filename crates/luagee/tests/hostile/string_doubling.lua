-- expect: error string length overflow
local s = "x" while true do s = s .. s end
