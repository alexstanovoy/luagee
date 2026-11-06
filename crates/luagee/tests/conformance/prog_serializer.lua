-- Program: deterministic serializer with sorted keys, string escaping and a round trip through loadstring.
local function serialize(v, indent)
  indent = indent or ""
  local t = type(v)
  if t == "string" then return string.format("%q", v) end
  if t == "number" then
    if v ~= v then return "0/0" end
    if v == math.huge then return "math.huge" end
    if v == -math.huge then return "-math.huge" end
    if v == math.floor(v) and math.abs(v) < 1e15 then return string.format("%d", v) end
    return string.format("%.17g", v)
  end
  if t ~= "table" then return tostring(v) end
  local keys = {}
  for k in pairs(v) do keys[#keys + 1] = k end
  table.sort(keys, function(a, b)
    if type(a) == type(b) and (type(a) == "number" or type(a) == "string") then return a < b end
    return type(a) < type(b)
  end)
  local parts = {}
  for _, k in ipairs(keys) do
    local key = type(k) == "string" and k:match("^[%a_][%w_]*$") and k or "[" .. serialize(k) .. "]"
    parts[#parts + 1] = indent .. "  " .. key .. " = " .. serialize(v[k], indent .. "  ")
  end
  if #parts == 0 then return "{}" end
  return "{\n" .. table.concat(parts, ",\n") .. "\n" .. indent .. "}"
end
local data = {
  name = "probe", level = 7, ratio = 0.1, big = 2^53, neg = -3.5, flag = true, empty = {},
  list = {1, 2, 3, "four", {5, 6}},
  ["with space"] = "x\ny\t\"q\"\\", [10] = "ten", [-1] = "minus", [1.5] = "frac",
  nested = {a = {b = {c = {d = "deep"}}}},
  inf = math.huge, ninf = -math.huge,
}
local text = serialize(data)
print(text)
local back = loadstring("return " .. text)()
print(serialize(back) == text)
print(back.list[5][2], back.nested.a.b.c.d, back["with space"] == data["with space"], back.ratio == 0.1, back[1.5])
local function deep_equal(a, b)
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then return a == b or (a ~= a and b ~= b) end
  for k, v in pairs(a) do if not deep_equal(v, b[k]) then return false end end
  for k in pairs(b) do if a[k] == nil then return false end end
  return true
end
print(deep_equal(data, back), deep_equal({1}, {1, 2}), deep_equal({a = {b = 1}}, {a = {b = 1}}))
local numbers = {0, 1, -1, 0.5, 1e15, 1e16, 123456789.123, 1/3, 2^-20, 1e-300, 255, -255, 2^31, 2^32}
local out = {}
for i, n in ipairs(numbers) do out[i] = serialize(n) end
print(table.concat(out, " "))
