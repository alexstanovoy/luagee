-- expect: error string length overflow
return select(2, (("x"):rep(4e6)):gsub("", "-"))
