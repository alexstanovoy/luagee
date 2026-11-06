-- expect: limit fuel_per_call
local name = ("x"):rep(4e6) while true do pcall(require, name) end
