-- table.sort with and without comparator on numbers, strings, records, large arrays, with a strict ordering only.
local function show(t) return table.concat(t, ",") end
local t = {5, 2, 8, 1, 9, 3}
table.sort(t) print(show(t))
table.sort(t, function(a, b) return a > b end) print(show(t))
local s = {"banana", "apple", "Cherry", "date", "Apple", "10", "9", "", "a"}
table.sort(s) print(show(s))
local words = {"b", "a", "c"}
table.sort(words, function(x, y) return x > y end) print(show(words))
local recs = {{n = "x", v = 3}, {n = "y", v = 1}, {n = "z", v = 2}}
table.sort(recs, function(a, b) return a.v < b.v end)
print(recs[1].n, recs[2].n, recs[3].n)
local big = {}
local seed = 12345
for i = 1, 2000 do seed = (seed * 1103515245 + 12345) % 2147483648 big[i] = seed % 1000 end
table.sort(big)
local ok = true
for i = 2, #big do if big[i - 1] > big[i] then ok = false end end
print(ok, #big)
table.sort(big, function(a, b) return a > b end)
ok = true
for i = 2, #big do if big[i - 1] < big[i] then ok = false end end
print(ok)
local already = {1, 2, 3, 4, 5}
table.sort(already) print(show(already))
local reversed = {5, 4, 3, 2, 1}
table.sort(reversed) print(show(reversed))
local same = {7, 7, 7, 7}
table.sort(same) print(show(same))
local single = {1}
table.sort(single) print(show(single))
local empty = {}
table.sort(empty) print(#empty)
local floats = {0.5, -1.5, 3, -0, 2^40, -2^40, 1e-9}
table.sort(floats) print(show(floats))
local pairs_list = {{1, "b"}, {1, "a"}, {0, "z"}, {2, "c"}}
table.sort(pairs_list, function(a, b) if a[1] ~= b[1] then return a[1] < b[1] end return a[2] < b[2] end)
local out = {}
for i, p in ipairs(pairs_list) do out[i] = p[1] .. p[2] end
print(show(out))
print(pcall(table.sort, {1, "a", 2}) == false, pcall(table.sort, {{}, {}}) == false, pcall(table.sort, nil) == false)
print(pcall(table.sort, {3, 2, 1}, function(a, b) error("cmp") end) == false)
local keys = {}
for k in pairs({z = 1, a = 2, m = 3, [1] = 4}) do keys[#keys + 1] = tostring(k) end
table.sort(keys) print(show(keys))
local by_len = {"ccc", "a", "bb", "dddd"}
table.sort(by_len, function(a, b) return #a < #b end) print(show(by_len))
local mixed_case = {"b", "B", "a", "A"}
table.sort(mixed_case, function(a, b) return a:lower() < b:lower() or (a:lower() == b:lower() and a < b) end) print(show(mixed_case))
local calls = 0
table.sort({3, 1, 2}, function(a, b) calls = calls + 1 return a < b end)
print(calls > 0)
local nan_free = {3, 1, 2}
table.sort(nan_free, function(a, b) return a > b end)
print(show(nan_free))
local desc_strings = {"10", "9", "100", "1"}
table.sort(desc_strings, function(a, b) return tonumber(a) > tonumber(b) end) print(show(desc_strings))
local with_holes_range = {}
for i = 1, 10 do with_holes_range[i] = (i * 7) % 10 end
table.sort(with_holes_range) print(show(with_holes_range))
