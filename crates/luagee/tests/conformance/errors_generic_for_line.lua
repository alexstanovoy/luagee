-- The iterator of a generic for is called on the line of the first value after `in`, as LuaJIT reports it.
local function line_of(source)
  local ok, message = pcall(loadstring(source))
  return message
end
print(line_of("\n\n for k,v in \n 3 \n do \n print(k) \n end"))
print(line_of("\n local a \n for k,v in 3 \n do \n print(k) \n end"))
print(line_of("for k in\n\n  nil,\n  1 do end"))
print(line_of("local t\nt = setmetatable({}, {__call = function() error('in the iterator') end})\nfor k in\n t do end"))
