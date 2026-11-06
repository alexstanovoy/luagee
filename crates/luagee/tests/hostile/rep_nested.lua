-- expect: error string length overflow
return #("x"):rep(1e6):rep(1e6)
