for i = 1, 20 do
    local t = host.collect(function() collectgarbage() end, i)
    assert(host.read(t) == i)
    host.remember(t)
end
collectgarbage()
assert(host.read(host.recall()) == 20)
host.forget()
