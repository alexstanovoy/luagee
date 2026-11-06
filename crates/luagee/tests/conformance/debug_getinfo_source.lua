-- debug.getinfo's source is the chunk's name as it was given: the name passed to load, or the source of an unnamed
-- string chunk, "=(load)" for an unnamed reader; short_src is that name as error positions print it.
local function show(f)
  local info = debug.getinfo(f)
  print(info.source, info.short_src, info.what)
end
show(load(function() return nil end, "modname"))
show(loadstring("return 1", "=x"))
show(loadstring("return 1"))
show(loadstring("return 1", "@some/file.lua"))
show(load("return 2"))
local pieces = {"return ", "3"}
local i = 0
show(load(function() i = i + 1 return pieces[i] end))
local nested = loadstring("return function() return 4 end", "=outer")()
collectgarbage()
show(nested)
show(print)
print(debug.getinfo(1).source:sub(1, 1))
