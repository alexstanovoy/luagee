-- An xpcall message handler runs where the error happened, before the failed call's frames are gone (so a traceback
-- shows them), and an error inside it is "error in error handling", as in LuaJIT. Tracebacks are printed without
-- LuaJIT's native frames ([C] lines), which luagee does not list.
local function lua_frames(text)
  local lines = {}
  for line in text:gmatch("[^\n]+") do
    if not line:find("^\t%[C%]") then lines[#lines + 1] = line end
  end
  return table.concat(lines, "\n")
end
local function inner() error("deep") end
local function middle() inner() end
print(lua_frames(select(2, xpcall(function() middle() end, debug.traceback))))
print(lua_frames(select(2, xpcall(middle, function(m) return debug.traceback("handled: " .. m) end))))
-- Through a native that called Lua: the comparator's frame is still there.
print(lua_frames(select(2, xpcall(function() table.sort({3, 2, 1}, function(a, b) error("in sort") end) end, debug.traceback))))
print(xpcall(error, error))
print(xpcall(function() error("a") end, function(m) error("b") end))
print(xpcall(function() error("a") end, function(m) local t = nil return t.x end))
print(pcall(xpcall, function() error("a") end, function(m) error({}) end))
print(xpcall(function() return "fine" end, error))
local calls = 0
print(xpcall(function() error("once") end, function(m) calls = calls + 1 return m .. "!" end), calls)
print(lua_frames(select(2, xpcall(middle, function(m) local text = debug.traceback("not a tail call") return text end))))
