-- expect: error nil value
local s = string.dump(function() return "bytecode ran" end) local done = false return load(function() if done then return nil end done = true return s end)()
