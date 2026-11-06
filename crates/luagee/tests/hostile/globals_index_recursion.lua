-- expect: error stack overflow
setmetatable(_G, {__index = function(t, k) return t[k] end}) return undefined_global
