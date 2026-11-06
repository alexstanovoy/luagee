-- expect: limit module_size
local first = true return load(function() if first then first = false return "return 0" end return "+1" end)
