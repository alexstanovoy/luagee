-- while, repeat (until sees the body's locals), break, numeric for edge cases, loop variable freshness.
local acc
local n = 0
while n < 5 do n = n + 1 end
print(n)
n = 0
repeat local done = n >= 3; n = n + 1 until done
print(n)
n = 0
while true do n = n + 1 if n > 10 then break end end
print(n)
repeat n = n - 1 if n == 5 then break end until false
print(n)
for i = 1, 3 do acc = nil; acc = i end
print(acc)
acc = nil
for i = 1, 0 do print("never") end
for i = 3, 1 do print("never") end
for i = 1, 3 do local t = i; i = i * 10; acc = i end
print(acc)
acc = nil
local out = {}
for i = 3, 1, -1 do out[#out + 1] = i end
print(table.concat(out, ","))
out = {}
for i = 1, 2, 0.25 do out[#out + 1] = i end
print(table.concat(out, ","))
out = {}
for i = 1, 0, -0.5 do out[#out + 1] = i end
print(table.concat(out, ","))
out = {}
for i = 0.1, 0.5, 0.1 do out[#out + 1] = string.format("%.1f", i) end
print(table.concat(out, ","))
out = {}
for i = 1, 10, 4 do out[#out + 1] = i end
print(table.concat(out, ","))
out = {}
for i = 10, 1, -4 do out[#out + 1] = i end
print(table.concat(out, ","))
out = {}
for i = 1, 3 do out[#out + 1] = i; i = 100 end
print(table.concat(out, ","))
out = {}
for i = "1", "3" do out[#out + 1] = i + 0 end
print(table.concat(out, ","))
print(pcall(function() for i = 1, "x" do end end) == false)
print(pcall(function() for i = 1, 10, 0 do break end end))
print(pcall(function() for i = nil, 10 do end end) == false)
print(pcall(function() for i = 1, {} do end end) == false)
out = {}
for i = 1, 3 do out[i] = function() return i end end
print(out[1](), out[2](), out[3]())
out = {}
local j = 0
while j < 3 do j = j + 1; local k = j; out[j] = function() k = k + 1 return k end end
print(out[1](), out[1](), out[2](), out[3]())
out = {}
for i = 1, math.huge do out[#out + 1] = i if i == 3 then break end end
print(#out)
local c = 0
for i = 1, -math.huge do c = c + 1 end
print(c)
for i = 1, 0/0 do c = c + 1 end
print(c)
for i = 0/0, 1 do c = c + 1 end
print(c)
for i = math.huge, 1 do c = c + 1 end
print(c)
for i = -math.huge, -math.huge + 1, 1 do c = c + 1 if c > 100 then break end end
print(c > 100)
for i = 1, 3 do for j = 1, 3 do if j == 2 then break end c = c + 100 end end
print(c)
for i = 2^53, 2^53 + 2 do c = c + 1 if c > 1000 then break end end
print(c > 700)
for i = 5, 5 do c = i end
print(c)
for i = -3, -1 do acc = i end
print(acc)
acc = nil
