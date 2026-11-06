-- expect: error nil value
return package.loadlib("libc.so.6", "system")
