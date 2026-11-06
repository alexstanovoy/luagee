-- expect: limit fuel_per_call
while true do local a, b = {}, {} a.b, b.a = b, a end
