-- expect: error index
getmetatable("").__index = nil return ("x"):len()
