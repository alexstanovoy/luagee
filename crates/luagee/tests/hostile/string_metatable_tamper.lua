-- expect: error call
getmetatable("").__index = function() return "tampered" end return ("x"):len()
