for _, x in ipairs({false, {}, function() end}) do
    pcall(host.convert, x)
    pcall(host.read, x)
    pcall(host.object_value, x)
end
pcall(host.bad_key, 0/0)
pcall(host.bad_key, nil)
pcall(host.invoke, function() error({}) end)
host.invoke(function() return 1 end)
