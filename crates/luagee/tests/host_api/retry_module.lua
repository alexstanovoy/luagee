package.preload.host_retry = function() return host.exhaust_once() end
assert(require('host_retry') == 71)
