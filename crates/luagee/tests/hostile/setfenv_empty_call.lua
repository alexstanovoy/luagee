-- expect: error nil value
setfenv(1, {}) print("x")
