-- expect: limit fuel_per_call
coroutine.wrap(function() while true do end end)() print("survived")
