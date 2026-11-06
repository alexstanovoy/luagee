-- Program: class hierarchy with metatables, inheritance, tostring, operators and a small simulation.
local function class(parent)
  local c = {}
  c.__index = c
  if parent then setmetatable(c, {__index = parent, __call = function(cls, ...) local o = setmetatable({}, cls) if cls.init then cls.init(o, ...) end return o end})
  else setmetatable(c, {__call = function(cls, ...) local o = setmetatable({}, cls) if cls.init then cls.init(o, ...) end return o end}) end
  c.super = parent
  return c
end
local Node = class()
function Node:init(name, load) self.name, self.load = name, load end
function Node:wear(d) self.load = self.load - d return self.load <= 0 end
function Node:describe() return self.name .. "(" .. self.load .. ")" end
Node.__tostring = function(u) return "Node:" .. u:describe() end
local Member = class(Node)
function Member:init(name, load, level) Node.init(self, name, load) self.level = level end
function Member:describe() return Node.describe(self) .. "@L" .. self.level end
Member.__tostring = Node.__tostring
Member.__lt = function(a, b) return a.level < b.level end
Member.__le = function(a, b) return a.level <= b.level end
Member.__eq = function(a, b) return a.name == b.name end
Member.__add = function(a, b) return Member(a.name .. "+" .. b.name, a.load + b.load, a.level + b.level) end
local u = Node("leaf", 10)
local h1, h2 = Member("ann", 100, 5), Member("bob", 120, 3)
print(tostring(u), tostring(h1), h1:describe(), u:describe())
print(h1 < h2, h1 > h2, h1 <= h2, h1 == Member("ann", 1, 1), h1 ~= h2)
print(tostring(h1 + h2), (h1 + h2).level)
print(u:wear(4), u:wear(6), u.load)
print(Member.super == Node, Node.super, getmetatable(h1) == Member, rawget(h1, "describe"), h1.wear == Node.wear)
local group = {Member("a", 1, 9), Member("b", 1, 2), Member("c", 1, 5)}
table.sort(group, function(x, y) return x < y end)
local names = {}
for i, h in ipairs(group) do names[i] = h.name .. h.level end
print(table.concat(names, " "))
local Stack = class()
function Stack:init() self.items, self.n = {}, 0 end
function Stack:push(...) for i = 1, select("#", ...) do self.n = self.n + 1 self.items[self.n] = (select(i, ...)) end return self end
function Stack:pop() local v = self.items[self.n] self.items[self.n] = nil self.n = self.n - 1 return v end
local st = Stack():push(1, 2, 3):push(4)
print(st:pop(), st:pop(), st.n)
local Counter = class()
Counter.count = 0
function Counter:init() Counter.count = Counter.count + 1 self.id = Counter.count end
local c1, c2, c3 = Counter(), Counter(), Counter()
print(c1.id, c2.id, c3.id, Counter.count, c1.count)
c1.count = 100
print(c1.count, Counter.count, c2.count)
