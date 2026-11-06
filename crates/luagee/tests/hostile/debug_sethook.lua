-- expect: error nil value
return debug.sethook(function() end, "l")
