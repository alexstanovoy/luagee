-- expect: error string length overflow
return #string.rep("x", 2^40)
