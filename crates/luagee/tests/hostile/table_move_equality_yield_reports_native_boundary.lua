-- expect: ok attempt to yield across a C-call boundary
-- This pins the native-library limitation; LuaJIT can yield in this equality handler.
local meta={__eq=function() coroutine.yield('yielded') end}
local a,b=setmetatable({},meta),setmetatable({},meta)
local co=coroutine.create(function() table.move(a,1,2,2,b) end)
local ok,message=coroutine.resume(co)
assert(not ok)
assert(coroutine.status(co)=='dead')
print(message:match('attempt to yield across a C%-call boundary'))
