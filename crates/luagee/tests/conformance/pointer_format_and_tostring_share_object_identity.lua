-- Compare only identity relations: LuaJIT reference addresses are not deterministic.
for _,object in ipairs({{},function() end,coroutine.create(function() end)}) do
 local pointer=string.format('%p',object)
 assert(tostring(object):match('0x%x+$')==pointer)
 print(type(object),'same identity')
end
