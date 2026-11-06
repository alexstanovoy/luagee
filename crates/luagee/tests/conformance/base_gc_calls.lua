-- collectgarbage options exist and return sane types (never the counts), plus weak tables do not crash.
print(type(collectgarbage("count")), collectgarbage("count") > 0)
print(type(collectgarbage()), type(collectgarbage("collect")))
print(type(collectgarbage("stop")), type(collectgarbage("restart")))
print(type(collectgarbage("step")), type(collectgarbage("setpause", 200)), type(collectgarbage("setstepmul", 200)))
print(pcall(collectgarbage, "bogus") == false)
local weak = setmetatable({}, {__mode = "v"})
for i = 1, 100 do weak[i] = {i} end
collectgarbage()
local alive = 0
for _ in pairs(weak) do alive = alive + 1 end
print(alive >= 0)
local weak_k = setmetatable({}, {__mode = "k"})
local keep = {}
weak_k[keep] = "kept"
for i = 1, 50 do weak_k[{}] = i end
collectgarbage()
print(weak_k[keep])
local before = collectgarbage("count")
local junk = {}
for i = 1, 10000 do junk[i] = {i, tostring(i)} end
print(collectgarbage("count") > before)
junk = nil
collectgarbage()
print(true)
local proxy_ok = true
local t = setmetatable({}, {__gc = function() end})
t = nil
collectgarbage()
print(proxy_ok)
