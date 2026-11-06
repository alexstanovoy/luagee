-- load calls its reader until it returns nil or an empty string; an error in the reader is load's failure: nil and
-- the error value. (Only the first line of a message is printed: LuaJIT's standalone interpreter adds a traceback.)
local function show(f, e)
  print(f, type(e), type(e) == "string" and e:match("^[^\n]*") or e == nil)
end
show(load(function() error("hhi") end))
show(load(function() error({}) end))
show(load(function() error() end))
local calls = 0
show(load(function() calls = calls + 1 if calls == 2 then error("second", 0) end return "return 1" end))
show(load(function() return {} end))
local pieces = {"return ", "1 ", "+ 2"}
local i = 0
local f = load(function() i = i + 1 return pieces[i] end, "=pieces")
print(f())
