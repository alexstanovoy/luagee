-- expect: error string length overflow
return ("x"):rep(1e9, ",")
