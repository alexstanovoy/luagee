local previous = host.object(17)
host.save_unrooted(previous)
previous = nil
collectgarbage()
local replacement = host.object(29)
assert(host.read_unrooted() == nil)
assert(not pcall(host.result_unrooted))
assert(not pcall(host.error_unrooted))
assert(host.object_value(replacement) == 29)
