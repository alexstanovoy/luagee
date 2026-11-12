-- LuaJIT quotes the string rendering of every type, including nil and booleans.
for _, value in ipairs({false, true, 42, -0.0, 0/0, math.huge, 'a"\\\0\n\1279'}) do
 print(string.format('%q',value))
end
print(string.format('%q',nil))
local meta=getmetatable(''); meta.__tostring=function() error('must not run') end
assert(string.format('%q','original')=='"original"'); meta.__tostring=nil
local values={false,true,42,'a"\\\0\n', {}}
for _, value in ipairs(values) do
 local object=setmetatable({}, {__tostring=function() return value end})
 local quoted=string.format('%q',object)
 if type(value)=='table' then print(quoted:match('^"table: 0x%x+"$')~=nil)
 else print(quoted) end
end
local object=setmetatable({}, {__tostring=function() return nil end})
print(string.format('%q',object))
local trapped=setmetatable({}, {__tostring=function() error('quote failure',0) end})
local ok,err=pcall(string.format,'%q',trapped); print(ok,err)
local ok,err=pcall(string.format,'%q'); print(ok,err:match('bad argument.*'))
local function shape(value,kind)
 local text=string.format('%q',value)
 assert(text:match('^"'..kind..':'))
 assert(text:sub(-1)=='"')
end
shape({},'table'); shape(function() end,'function'); shape(coroutine.create(function() end),'thread')
print('object renderings are quoted')
for byte=0,255 do print(byte, string.format('%q',string.char(byte)..'9')) end
