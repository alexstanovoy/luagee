-- Program: text processing (word frequency, run-length encoding, base64 with bit ops, caesar, formatting tables).
local text = "the quick brown fox jumps over the lazy dog the end"
local freq = {}
for w in text:gmatch("%a+") do freq[w] = (freq[w] or 0) + 1 end
local words = {}
for w, n in pairs(freq) do words[#words + 1] = {w = w, n = n} end
table.sort(words, function(a, b) if a.n ~= b.n then return a.n > b.n end return a.w < b.w end)
for i = 1, 4 do print(words[i].w, words[i].n) end
local function rle(s) return (s:gsub("(.)%1*", function(c) return c end)) end
local function rle_encode(s)
  local out, i = {}, 1
  while i <= #s do
    local c, j = s:sub(i, i), i
    while s:sub(j + 1, j + 1) == c do j = j + 1 end
    out[#out + 1] = (j - i + 1) .. c
    i = j + 1
  end
  return table.concat(out)
end
local function rle_decode(s) return (s:gsub("(%d+)(%D)", function(n, c) return c:rep(tonumber(n)) end)) end
print(rle_encode("aaabccdddd"), rle_decode(rle_encode("aaabccdddd")), rle_encode(""), rle_encode("x"))
local b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local function encode64(s)
  local out = {}
  for i = 1, #s, 3 do
    local a, b, c = s:byte(i, i + 2)
    local n = bit.bor(bit.lshift(a, 16), bit.lshift(b or 0, 8), c or 0)
    local c1, c2, c3, c4 = bit.band(bit.rshift(n, 18), 63), bit.band(bit.rshift(n, 12), 63), bit.band(bit.rshift(n, 6), 63), bit.band(n, 63)
    out[#out + 1] = b64:sub(c1 + 1, c1 + 1) .. b64:sub(c2 + 1, c2 + 1) .. (b and b64:sub(c3 + 1, c3 + 1) or "=") .. (c and b64:sub(c4 + 1, c4 + 1) or "=")
  end
  return table.concat(out)
end
print(encode64("Man"), encode64("Ma"), encode64("M"), encode64("hello world"), encode64(""), encode64("\0\255\128"))
local function caesar(s, k) return (s:gsub("%a", function(c) local base = c:match("%l") and 97 or 65 return string.char((c:byte() - base + k) % 26 + base) end)) end
print(caesar("Hello, World!", 3), caesar(caesar("Hello, World!", 3), -3), caesar("xyz", 5))
local rows = {{"name", "qty", "price"}, {"apple", 3, 1.5}, {"watermelon", 12, 0.25}}
for _, r in ipairs(rows) do print(string.format("%-12s|%5s|%8s", r[1], tostring(r[2]), tostring(r[3]))) end
local csv = "a,b,,d\n1,2,3,4\n"
local lines = {}
for line in csv:gmatch("[^\n]+") do
  local fields = {}
  for field in (line .. ","):gmatch("([^,]*),") do fields[#fields + 1] = field end
  lines[#lines + 1] = #fields .. ":" .. table.concat(fields, "|")
end
print(table.concat(lines, " "))
local function palindrome(s) s = s:lower():gsub("[^%w]", "") return s == s:reverse() end
print(palindrome("A man, a plan, a canal: Panama"), palindrome("hello"), palindrome(""))
local function wrap(s, width)
  local lines2, line = {}, ""
  for word in s:gmatch("%S+") do
    if #line + #word + 1 > width and #line > 0 then lines2[#lines2 + 1] = line line = word
    else line = (#line > 0 and line .. " " or "") .. word end
  end
  lines2[#lines2 + 1] = line
  return lines2
end
print(table.concat(wrap(text, 16), "/"))
local template = "Hello ${name}, you are ${age} years old. ${missing}"
print((template:gsub("%${(%w+)}", {name = "Bob", age = 42})))
local hex = ("hello"):gsub(".", function(c) return string.format("%02x", c:byte()) end)
print(hex, (hex:gsub("%x%x", function(h) return string.char(tonumber(h, 16)) end)))
print(("%s has %d items costing %.2f"):format("cart", 3, 9.5), ("x"):rep(3, ", "))
