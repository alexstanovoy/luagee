-- expect: limit fuel_per_call
while true do pcall(function() while true do end end) end
