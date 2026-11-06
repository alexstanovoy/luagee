-- Table semantics: key normalisation, nil/NaN keys, nested constructors, length of sequences, reference identity.
local t = {}
t[1.0] = "one" t[2^53] = "big" t["1"] = "string one"
print(t[1], t["1"], #t, t[2^53], t[2^53 + 0.0])
t[1.5] = "frac"
print(t[1.5], t[3 / 2], t[0.5 + 1])
t[-0] = "zero" 
print(t[0], t[-0], t[0.0], t[0/1])
t[true] = "bool" t[false] = "nf"
print(t[true], t[false], t[1 == 1], t[1 == 2])
local k1, k2 = {}, {}
t[k1] = "first" t[k2] = "second"
print(t[k1], t[k2], t[{}])
t[print] = "fn"
print(t[print])
t[math.huge] = "inf" t[-math.huge] = "-inf"
print(t[math.huge], t[-math.huge], t[1 / 0])
local c = {1, 2, 3, x = 1, y = 2, [10] = 10, [3.0] = "three", "four"}
print(c[1], c[2], c[3], c[4], c.x, c[10], #c)
local nested = {{1, 2}, {3, 4, {5, 6, {7}}}}
print(nested[2][3][3][1], #nested, #nested[2], #nested[2][3])
local trailing = {1, 2, 3,}
local semi = {1; 2; 3;}
local mixed = {1, 2; 3, n = 4; 5}
print(#trailing, #semi, #mixed, mixed.n)
local order = {}
local function mark(x) order[#order + 1] = x return x end
local ord = {mark("a"), mark("b"), k = mark("c"), mark("d")}
print(table.concat(order, ""))
print(#{n = 1}, #{"a", "b"})
local a = {} local b = a
a.x = 1
print(b.x, a == b, {} == {}, rawequal(a, b))
local function modify(tt) tt.changed = true end
modify(a)
print(b.changed)
local cnt = 0
for _ in pairs({1, 2, 3, a = 1, b = 2}) do cnt = cnt + 1 end
print(cnt)
local arr = {}
for i = 1, 10 do arr[i] = i end
for i = 1, 10 do arr[i] = nil end
print(#arr, next(arr))
local sparse = {}
sparse[1000000] = 1
print(#sparse == 0 or #sparse == 1000000, sparse[1000000])
local ks = {}
local tbl = {}
for i = 1, 100 do tbl["key" .. i] = i end
for k, v in pairs(tbl) do ks[#ks + 1] = v end
table.sort(ks)
print(#ks, ks[1], ks[100])
for k in pairs(tbl) do tbl[k] = nil end
print(next(tbl))
local grow = {}
for i = 1, 1000 do grow[i] = i end
for i = 1000, 1, -1 do grow[i] = nil end
for i = 1, 10 do grow[#grow + 1] = i end
print(#grow)
print(pcall(function() local x = {} x[nil] = 1 end) == false, ({})[nil], rawget({}, nil))
print(pcall(function() local x = {} x[0/0] = 1 end) == false, ({})[0/0])
local neg = {[-1] = "m1", [0] = "z", "p1"}
print(neg[-1], neg[0], neg[1], #neg)
local str_keys = {["a b"] = 1, [""] = 2, ["1"] = 3, [" "] = 4}
print(str_keys["a b"], str_keys[""], str_keys["1"], str_keys[1], str_keys[" "])
local function f() return 1, 2, 3 end
local fr = {f(), f()}
print(#fr)
local self = {}
self.me = self
print(self.me.me.me == self)
local num_str = {}
num_str["10"] = "s" num_str[10] = "n"
print(num_str["10"], num_str[10])
local t2 = {}
t2[1] = 1 t2[2] = 2 t2[4] = 4
print(t2[3], t2[4], #t2 == 2 or #t2 == 4)
