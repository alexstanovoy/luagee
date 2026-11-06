local t = {}
for i = 1, 1000 do
    host.raw_set(t, i, i)
end
assert(host.raw_get(t, 1000) == 1000)
host.set_metatable(t, {__index = {fallback = 17}})
assert(t.fallback == 17)
host.raw_set(t, 'fallback', 29)
assert(host.raw_get(t, 'fallback') == 29)
assert(not pcall(host.raw_set, t, 0/0, 1))
assert(not pcall(host.raw_set, t, nil, 1))
host.set_metatable(t, nil)
