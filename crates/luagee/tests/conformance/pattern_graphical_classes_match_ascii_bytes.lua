-- LuaJIT extends Lua 5.1 with %g and %G; locale is C.
for byte = 0, 255 do
 local s = string.char(byte)
 local graphical = byte >= 33 and byte <= 126
 for _, pattern in ipairs({'%g', '[%g]', '[^%G]'}) do
  assert((s:match(pattern) ~= nil) == graphical, byte)
 end
 for _, pattern in ipairs({'%G', '[%G]', '[^%g]'}) do
  assert((s:match(pattern) ~= nil) ~= graphical, byte)
 end
end
for _, pattern in ipairs({'[%g', '[%G', '%'}) do
 local ok, err = pcall(string.match, '!', pattern)
 print(ok, err:match('malformed pattern.*'))
end
print('all 256 bytes and complements match')
