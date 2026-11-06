-- expect: ok false	cannot change a protected metatable
setmetatable(_G, {__metatable = "locked"}) print(pcall(setmetatable, _G, {}))
