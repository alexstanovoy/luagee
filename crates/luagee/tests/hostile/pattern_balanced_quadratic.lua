-- expect: limit fuel_per_call
return #(("("):rep(1e5)):gsub("%b()", "")
