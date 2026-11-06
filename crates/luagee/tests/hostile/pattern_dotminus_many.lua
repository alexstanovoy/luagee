-- expect: limit fuel_per_call
return string.match(("a"):rep(5000), ".-.-.-.-.-.-.-.-b")
