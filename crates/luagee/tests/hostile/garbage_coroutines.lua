-- expect: limit fuel_per_call
while true do local co = coroutine.create(function() coroutine.yield() end) coroutine.resume(co) end
