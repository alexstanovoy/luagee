-- expect: limit module_size
return loadstring("return 1" .. (" "):rep(2^21))
