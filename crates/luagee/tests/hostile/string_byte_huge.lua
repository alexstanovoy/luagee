-- expect: error too long
return select("#", (("x"):rep(2^20)):byte(1, -1))
