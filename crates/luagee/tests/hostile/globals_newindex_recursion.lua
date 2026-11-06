-- expect: error stack overflow
setmetatable(_G, {__newindex = function(t, k, v) t[k] = v end}) new_global = 1
