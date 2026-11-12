for _, marker in ipairs({'#', '#n', '#\0', '#anything', '#\255'}) do
 print(select(marker), select(marker, 'a', nil, 'b'))
end
for _, value in ipairs({'', 'n#', '\0#', 'invalid', false, {}}) do
 local ok, err = pcall(select, value, 1, 2)
 print(ok, err:match("bad argument.*"))
end
print(select('2', 10, 20, 30), select(-1, 10, 20))
for _, value in ipairs({0, -4, 1e300, 0/0, math.huge, -math.huge, -0.0}) do
 local ok, result = pcall(select, value, 10, 20)
 print(ok, result)
end
