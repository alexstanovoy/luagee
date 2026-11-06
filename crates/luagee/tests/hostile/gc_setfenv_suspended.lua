-- expect: ok old	new
local f = setfenv(function() while true do coroutine.yield(x) end end, {x = "old", coroutine = coroutine})
local co = coroutine.wrap(f)
local first = co()
setfenv(f, {x = "new", coroutine = coroutine})
collectgarbage() collectgarbage()
for i = 1, 1000 do local t = {x = "garbage"} end
collectgarbage()
print(first, co())
