-- Arithmetic: coercions of strings, % with negatives and floats, division by zero, -0, inf, nan, power.
local function try(f, ...) local ok, v = pcall(f, ...) if ok then return v end return "error" end
print(10 / 2, 7 / 2, -7 / 2, 1 / 3, 2 ^ 10, 2 ^ 0.5 == math.sqrt(2), 10 % 3, 10.5 % 3, -10 % 3, 10 % -3, -10 % -3)
print(5.5 % 2, -5.5 % 2, 5.5 % -2, -5.5 % -2, 0 % 5, 5 % 5, 5 % 1, 5.25 % 1, -5.25 % 1)
print(1 / 0, -1 / 0, 0 / 0 ~= 0 / 0, 1 / (1 / 0), 1 / -(1 / 0), 5 % math.huge, -5 % math.huge, 5 % -math.huge, -5 % -math.huge)
print(try(function() return 5 % 0 end) ~= 3, 5 % 0 ~= 5 % 0, math.huge % 5 ~= math.huge % 5)
print(0 * -1, -0, 0 / -1, 1 / (0 * -1), 1 / -0, 1 / (-0), 0 == -0, -0 == 0)
print(math.huge + 1, math.huge - math.huge ~= 0, math.huge * 0 ~= 0, -math.huge < math.huge, math.huge == math.huge)
print("10" + 5, "0x10" + 0, "1e1" + 0, " 5 " * 2, "5" * "2", 10 .. 20, "3" ^ "2", -"3", - "-3")
print(try(function() return "abc" + 1 end), try(function() return {} + 1 end), try(function() return nil + 1 end))
print(try(function() return "" + 1 end), try(function() return "1 2" + 1 end), try(function() return true + 1 end))
print(2 ^ 53, 2 ^ 53 + 1, 2 ^ 63, 2 ^ 64, -2 ^ 63, 2 ^ 31, 2 ^ 32, 2 ^ -1074, 2 ^ -1075, 2 ^ 1023 * 2)
print(3 % 2 ^ 2, 1e15 + 0.5, 1e16 + 1, 0.1 + 0.2, 0.1 + 0.2 == 0.3, 1 - 0.9, 100 * 1.1)
print(1e300 * 1e10, -1e300 * 1e10, 1e-300 * 1e-300, 4.9e-324 / 2, 2^0.5 * 2^0.5)
print(0.1 * 3, 1/3 * 3, 3 * 1/3, 1e15 / 3, 123456789 * 987654321, 99999999999 * 99999999999)
print(255 % 256, -1 % 256, 2^32 % 7, 2^53 % 3, -2^53 % 3, 1e20 % 7, 5.3 % 0.1 < 0.1)
print(8 % 3.5, -8 % 3.5, 8 % -3.5, 2^0, 0^0, 0^-1, (-8)^(1/3) ~= (-8)^(1/3), (-2)^2, (-2)^3, (-2)^0.5 ~= (-2)^0.5)
print(math.huge ^ 0, 1 ^ math.huge, 1 ^ (0/0) , 0/0 ^ 0)
local x = 5
x = x + 1; x = x * 2; x = x - 1; x = x / 11; x = x ^ 2
print(x)
print(3 == 3.0, 2^53 == 2^53 + 1, 1e308 * 10 == math.huge, "1" + "1" == 2, 10 == "10")
print(1 < 1.5, 1.5 < 2, -1 < -0.5, 0 < -0, -0 < 0, 0/0 < 1, 0/0 > 1, 0/0 == 0/0, 0/0 <= 0/0, 0/0 >= 0/0)
print(math.huge > 2^1000, -math.huge < -2^1000, math.huge <= math.huge, math.huge < math.huge)
