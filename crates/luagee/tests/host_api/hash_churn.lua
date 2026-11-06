local t = {}
for i = 100000, 101023 do host.raw_set(t, i, true) end
for i = 0, 511 do
    host.raw_set(t, 100000 + i, nil)
    host.raw_set(t, 200000 + i, true)
end
local migrated = {}
for i = 2, 2000, 2 do host.raw_set(migrated, i, i) end
for i = 1, 1999, 2 do host.raw_set(migrated, i, i) end
assert(#migrated == 2000)
