-- Method calls through a metatable, closures and filtering over a list of small tables.
local Point = {}
Point.__index = Point
function Point.new(x, y, group, weight) return setmetatable({ x = x, y = y, group = group, weight = weight }, Point) end
function Point:Position() return { x = self.x, y = self.y } end
function Point:Group() return self.group end
function Point:Live() return self.weight > 0 end
local points = {}
for i = 1, 40 do points[i] = Point.new((i * 37) % 900, (i * 91) % 900, i % 2, (i * 13) % 100) end
local function near(me, radius, others)
  local out = {}
  local p = me:Position()
  for _, u in ipairs(points) do
    if u ~= me and u:Live() and (u:Group() ~= me:Group()) == others then
      local q = u:Position()
      local dx, dy = q.x - p.x, q.y - p.y
      if dx * dx + dy * dy <= radius * radius then out[#out + 1] = u end
    end
  end
  return out
end
local function filter(list, pred) local r = {} for _, v in ipairs(list) do if pred(v) then r[#r + 1] = v end end return r end
local total = 0
for step = 1, 20000 do
  local me = points[step % 40 + 1]
  local others = near(me, 500, true)
  local light = filter(others, function(u) return u.weight < 50 end)
  total = total + #others * 3 + #light
  me.weight = (me.weight + 7) % 100
end
print(total)
