-- LuaJIT %p identities vary by process: compare relations, never reference addresses.
local values={'same', 'sa'..'me', {}, {}, function() end, function() end, coroutine.create(function() end), print}
for _, value in ipairs({false, true, 0, -0.0, 0/0, math.huge}) do
 print(string.format('%p',value))
end
for _, template in ipairs({'%p','%20p','%-20p','%.2p','%020p','%+p','%#p'}) do
 print(string.format(template,nil))
end
local identities={}
for i,value in ipairs(values) do
 local id=string.format('%p',value)
 assert(id:match('^0x%x+$'))
 assert(id==string.format('%p',value))
 for j=1,i-1 do print(i,j,id==identities[j]) end
 identities[i]=id
end
local trapped=setmetatable({},{__tostring=function() error('must not stringify') end})
print(string.format('%p',trapped)==string.format('%p',trapped))
local ok,err=pcall(string.format,'%p'); print(ok,err:match('bad argument.*'))
