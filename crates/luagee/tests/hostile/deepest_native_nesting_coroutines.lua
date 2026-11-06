-- expect: ok done
-- Coroutines resumed inside coroutines to the native nesting bound (the heaviest native level there is), then, one
-- level short of it, the deepest source the parser and compiler accept and the deepest patterns the matcher runs:
-- the most native stack a script can make the interpreter use at once. The wasm gate runs it on less than wasm's
-- default stacks.
local levels = 199
local loops = string.rep("for i = 1, 2 do ", levels) .. string.rep(" end", levels)
local calls = "return " .. string.rep("f(", levels - 1) .. string.rep(")", levels - 1)
local functions = "return " .. string.rep("function() return ", 66) .. "1" .. string.rep(" end", 66)
local reached
local function level()
    if not pcall(coroutine.wrap(level)) and not reached then
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
level()
print(reached)
