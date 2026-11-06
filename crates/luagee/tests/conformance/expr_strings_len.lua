-- Length operator on strings and sequences, concatenation of numbers, string comparison, string coercions.
print(#"", #"a", #"hello", #"\0\0", #"héllo", #("x"):rep(1000))
print(#{}, #{1}, #{1, 2, 3}, #{n = 1}, #{"a", "b", "c", "d", "e"})
local t = {}
for i = 1, 100 do t[i] = i end
print(#t)
t[#t + 1] = 101
print(#t, t[#t])
t[#t] = nil
print(#t)
for i = 100, 51, -1 do t[i] = nil end
print(#t)
print(1 .. "", 1.0 .. "", 0.5 .. "|", -1 .. "", 1e3 .. "", 1e15 .. "", 1e16 .. "", 2^63 .. "", 123456789012 .. "")
print("a" .. 1 + 2, "n=" .. 10 / 4, "x" .. -1, "x" .. -(-1))
print(pcall(function() return "a" .. nil end) == false, pcall(function() return "a" .. {} end) == false, pcall(function() return "a" .. true end) == false)
print(("abc"):len(), ("abc").len == string.len, #"abc" == ("abc"):len())
print(("x"):rep(3), ("%d"):format(5), ("abc"):upper(), #("a"):rep(10, ","))
print("abc" == "abc", "abc" == "abd", "abc" ~= "abc", "" == "", "a" .. "b" == "ab")
print(tostring(12) == "12", tostring(1.5) == "1.5", 12 .. "" == "12", "12" + 0 == 12)
print(0.1 .. "", 1/3 .. "", 100 .. "", 1e100 .. "", 123456.789 .. "", 0.000001 .. "", 1e-7 .. "", 123456789012345678 .. "")
local s = "x"
for i = 1, 5 do s = s .. s end
print(#s)
local parts = {}
for i = 1, 10 do parts[#parts + 1] = i .. "" end
print(table.concat(parts, ","))
