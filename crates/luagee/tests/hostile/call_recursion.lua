-- expect: error stack overflow
local mt = {} mt.__call = function(self) return 1 + self() end return setmetatable({}, mt)()
