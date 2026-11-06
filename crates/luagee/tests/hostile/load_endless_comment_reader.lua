-- expect: limit module_size
local first = true return load(function() if first then first = false return "--" end return "x" end)
