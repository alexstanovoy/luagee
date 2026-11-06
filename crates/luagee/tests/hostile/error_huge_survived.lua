-- expect: ok survived
local message = ("x"):rep(1e6) for i = 1, 1e3 do pcall(error, message) end print("survived")
