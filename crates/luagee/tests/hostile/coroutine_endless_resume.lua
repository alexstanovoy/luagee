-- expect: limit fuel_per_call
local co = coroutine.create(function() while true do end end) print(coroutine.resume(co)) print("survived")
