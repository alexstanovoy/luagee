-- Base library: type, ipairs/pairs/next, unpack, select, loadstring, load with reader, rawequal, collectgarbage presence.
print(type(nil), type(true), type(1), type("s"), type({}), type(print), type(function() end), type(coroutine.create(print)))
print(pcall(type) == false, type(type))
print(next({}), next({10}), next({a = 1}))
print(next({10, 20}, 1), next({10, 20}, 2))
local t = {a = 1}
print(next(t, "a"), next(t))
print(select("#", next({})))
print(unpack({1, 2, 3}))
print(unpack({1, 2, 3}, 2))
print(unpack({1, 2, 3}, 2, 3))
print(unpack({1, 2, 3}, 0, 1))
print(unpack({1, 2, 3}, 3, 2))
print(select("#", unpack({}, 1, 5)), select("#", unpack({}, 5, 1)), select("#", unpack({n = 3})))
print(unpack({1, nil, 3}, 1, 3))
print(unpack({}, -1, 1))
print(unpack({"a", "b"}, 1.0, 2.0))
print(select("#", unpack({1, 2, 3}, 1, 100)))
print(pcall(unpack, nil) == false)
print(ipairs({}) == ipairs({}), type(ipairs({})))
local n = 0
for _ in ipairs({1, 2, nil, 4}) do n = n + 1 end
print(n)
n = 0
for _ in ipairs({[1] = 1, [2] = 2, [4] = 4}) do n = n + 1 end
print(n)
local f = loadstring("return 1 + 1")
print(type(f), f())
local f2, err = loadstring("return +")
print(f2, type(err), err:find("unexpected symbol") ~= nil)
local f3, err3 = loadstring("x =", "=custom")
print(f3, err3)
local f4, err4 = loadstring("x = = 1", "=name")
print(err4)
print(select(2, loadstring("error('boom')", "=chunk")) == nil, pcall(loadstring("error('boom')", "=chunk")))
print(select(2, pcall(loadstring("error('boom')", "@file.lua"))))
print(select(2, pcall(loadstring("\n\nerror('boom')", "=(load)"))))
print(select(2, pcall(loadstring("error('boom')", "plain code name"))))
print(select(2, pcall(loadstring("error('boom')"))))
print(loadstring("return ...", "=x")(1, 2, 3))
local pieces = {"return ", "10 ", "+ 5"}
local idx = 0
local g = load(function() idx = idx + 1 return pieces[idx] end)
print(g())
local empty = load(function() return nil end)
print(type(empty), empty())
local empty2 = load(function() return "" end)
print(type(empty2))
local bad = load(function() return {} end)
print(bad)
local split = {"ret", "urn 'sp", "lit'"}
local si = 0
print(load(function() si = si + 1 return split[si] end)())
print(load("return 1") == nil, pcall(load) == false)
print(loadstring("x = 1; return x")(), x)
x = nil
print(loadstring(" ")(), select("#", loadstring("")()))
print(type(collectgarbage), type(collectgarbage("count")), collectgarbage("count") > 0)
print(type(_VERSION), _VERSION)
print(rawequal(1, 1), rawequal("a", "a"), rawequal({}, {}), rawequal(0, -0), rawequal(0/0, 0/0))
local keys = {}
for k, v in pairs(_G) do keys[#keys + 1] = k end
print(#keys > 20)
print(#"" == 0, ("x"):rep(0) == "")
print(tostring(select("#", pcall(error))))
local mt = {}
print(getmetatable(setmetatable({}, mt)) == mt, type(getmetatable("")))
print(pcall(setmetatable, {}, {}) , rawget(_G, "undefined_thing"))
print(type(string), type(table), type(math), type(coroutine), type(bit), type(getfenv), type(setfenv), type(module), type(xpcall), type(unpack), type(loadstring), type(math.pow), type(collectgarbage), type(select), type(rawlen), type(table.maxn))
