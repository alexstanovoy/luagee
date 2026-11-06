-- table.insert/remove in both forms, table.concat with separators and ranges, maxn, border behaviour without holes.
local function show(t) return table.concat(t, ",") end
local t = {}
table.insert(t, "a") table.insert(t, "b") table.insert(t, 1, "c") table.insert(t, 2, "d") table.insert(t, #t + 1, "e")
print(show(t), #t)
print(table.remove(t), show(t))
print(table.remove(t, 1), show(t))
print(table.remove(t, 2), show(t))
print(table.remove(t, #t), show(t))
print(table.remove({}), table.remove({}, 0), #t)
print(select("#", table.remove({})), select("#", table.remove({}, 1)))
local r = {1, 2, 3}
print(table.remove(r, 4), show(r), table.remove(r, 3), show(r))
print(pcall(table.insert, {}, 5, "x") == false, pcall(table.insert, {1, 2}, 0, "x") == false, pcall(table.insert, {1, 2}, 4, "x") == false)
print(pcall(table.insert, {}) == false, pcall(table.insert, {}, 1, 2, 3) == false, pcall(table.insert, nil, 1) == false)
local big = {}
for i = 1, 100 do table.insert(big, i) end
for i = 1, 50 do table.remove(big, 1) end
print(#big, big[1], big[50])
for i = 1, 25 do table.insert(big, 1, -i) end
print(#big, big[1], big[25], big[26])
print(table.concat({}), table.concat({}, ","), table.concat({1}, ","), table.concat({1, 2, 3}), table.concat({1, 2, 3}, ", "))
print(table.concat({"a", "b", "c", "d"}, "-", 2), table.concat({"a", "b", "c", "d"}, "-", 2, 3), table.concat({"a", "b"}, "-", 2, 1) == "", table.concat({"a", "b"}, "-", 1, 1))
print(table.concat({1, 2.5, "x"}, "|"), table.concat({"a", "b"}, ""), table.concat({"a", "b"}, nil), table.concat({"a", "b"}, 5))
print(pcall(table.concat, {1, {}, 3}) == false, pcall(table.concat, {1, 2}, ",", 1, 3) == false, pcall(table.concat, {true}) == false)
print(table.concat({"a", "b", "c"}, ", ", 0 + 1, 3.0))
print(table.maxn({}), table.maxn({1, 2, 3}), table.maxn({[10] = 1}), table.maxn({1, nil, 3}), table.maxn({a = 1}))
local seq = {}
for i = 1, 10 do seq[i] = i end
seq[#seq] = nil
seq[#seq] = nil
print(#seq)
seq[#seq + 1] = "x"
print(#seq, seq[#seq])
local q = {}
local head, tail = 1, 0
for i = 1, 5 do tail = tail + 1 q[tail] = i end
local sum = 0
while head <= tail do sum = sum + q[head] q[head] = nil head = head + 1 end
print(sum, next(q))
local stack = {}
for i = 1, 5 do stack[#stack + 1] = i end
local popped = {}
while #stack > 0 do popped[#popped + 1] = table.remove(stack) end
print(show(popped))
local reuse = {1, 2, 3}
table.remove(reuse, 1) table.remove(reuse, 1) table.remove(reuse, 1)
print(#reuse, next(reuse))
local ins_nil = {1, 2, 3}
table.insert(ins_nil, 2, nil)
print(select("#", unpack(ins_nil, 1, 4)))
local ret = table.insert({}, 1)
print(ret, select("#", table.insert({}, 1)))
local with_meta = setmetatable({}, {__index = function() return "never" end})
table.insert(with_meta, "a")
print(with_meta[1], #with_meta)
