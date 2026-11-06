-- expect: ok done
-- Native calls nested to their bound (metamethods calling metamethods), then, one level short of it, the deepest
-- source the parser and compiler accept and the deepest pattern the matcher runs: the most native stack a script
-- can make the interpreter use through metamethods.
local levels = 199
local loops = string.rep("for i = 1, 2 do ", levels) .. string.rep(" end", levels)
local calls = "return " .. string.rep("f(", levels - 1) .. string.rep(")", levels - 1)
local functions = "return " .. string.rep("function() return ", 66) .. "1" .. string.rep(" end", 66)
local mt, reached = {}, nil
mt.__newindex = function()
    if not pcall(function() setmetatable({}, mt).x = 1 end) and not reached then
        reached = "the bound"
        return
    end
    if reached == "the bound" then
        reached = "done"
        assert(loadstring(loops))
        assert(loadstring(calls))
        assert(loadstring(functions))
        string.find(string.rep("a", 300), string.rep("a*", levels) .. "b")
        string.find(string.rep("a", 300), string.rep("a-", levels) .. "b")
    end
end
setmetatable({}, mt).x = 1
print(reached)
