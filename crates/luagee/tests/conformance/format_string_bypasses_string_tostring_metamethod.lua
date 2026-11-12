local meta=getmetatable('')
meta.__tostring=function() error('string handler must not run') end
assert(string.format('%s','original')=='original')
assert(string.format('[%10.3s]', 'original')=='[       ori]')
assert(string.format('%s','a\0b')=='a\0b')
assert(string.format('%s','')=='')
meta.__tostring=nil
for _, value in ipairs({42, false, true, 'text'}) do print(string.format('%s',value)) end
local t=setmetatable({}, {__tostring=function() error('handler failure',0) end})
local ok,err=pcall(string.format,'%s',t); print(ok,err)
for _, value in ipairs({false, true, 42, {}}) do
 local t=setmetatable({}, {__tostring=function() return value end})
 local s=string.format('%s',t)
 print(type(s), type(value)=='table' and s:match('^table: 0x%x+$')~=nil or s)
end
local t=setmetatable({}, {__tostring=function() return nil end})
print(string.format('%s',t))
local ok,err=pcall(string.format,'%s'); print(ok,err:match('bad argument.*'))
print('strings bypass their handler')
