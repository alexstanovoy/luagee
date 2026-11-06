-- expect: error nil value
return io.open("/etc/hostname"):read("*a")
