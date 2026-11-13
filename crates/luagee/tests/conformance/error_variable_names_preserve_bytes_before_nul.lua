-- LuaJIT formats variable/callee names as byte strings ending at their first NUL.
-- Comparisons name types, not variables. C0/DEL cannot occur inside source identifiers;
-- arbitrary bytes can occur in constant field keys and in names inferred for native calls.
local function hex(bytes)
 return (bytes:gsub('.',function(c) return string.format('%02x',string.byte(c)) end))
end
local function outcome(kind,name,chunk)
 local ok,message=pcall(chunk)
 assert(not ok and type(message)=='string')
 print(kind,hex(name),hex(message))
end
local function check(kind,name,source)
 outcome(kind,name,assert(loadstring(source,'=case')))
end
local operations={
 {'index','return %s.key'},
 {'assign','%s.key=1'},
 {'call','%s()'},
 {'arithmetic','return %s+1'},
 {'negate','return -%s'},
 {'length','return #%s'},
 {'concatenate','return %s..1'},
 {'compare','return %s<1'},
 {'method_receiver','return %s:missing()'},
}
local function field(name)
 -- Decimal escapes keep every generated case on one source line, including names containing LF.
 local literal='"'..name:gsub('.',function(c) return string.format('\\%03d',c:byte()) end)..'"'
 local source={}
 for _,operation in ipairs(operations) do
  source[#source+1]='function() local t={}; '..string.format(operation[2],'t['..literal..']')..' end'
 end
 source[#source+1]='function() local t={['..literal..']=math.abs}; t['..literal..'](false) end'
 -- Compile the cases together, keeping the fixture below the module-count limit.
 local chunks=assert(loadstring('return {'..table.concat(source,',')..'}','=case'))()
 for i,operation in ipairs(operations) do outcome('field '..operation[1],name,chunks[i]) end
 outcome('field native argument',name,chunks[#source])
end
-- A minimal version of the differential concatenation failure, then every byte.
field('\0z')
field('prefix\0suffix')
field('')
field("quote'\\name")
field(string.rep('n',80))
for byte=0,255 do field(string.char(byte)..'z') end
-- Locals, upvalues, globals, and methods permit extended bytes in identifiers, but not C0/DEL.
for _,name in ipairs({'named','n_\128z','n_\255z'}) do
 for _,operation in ipairs(operations) do
  local action=string.format(operation[2],name)
  check('local '..operation[1],name,'local '..name..'; '..action)
  check('upvalue '..operation[1],name,'local '..name..'; return (function() '..action..' end)()')
  check('global '..operation[1],name,name..'=nil; '..action)
 end
 check('local native argument',name,'local '..name..'=math.abs; '..name..'(false)')
 check('upvalue native argument',name,'local '..name..'=math.abs; return (function() '..name..'(false) end)()')
 check('global native argument',name,name..'=math.abs; '..name..'(false)')
 check('method call',name,'local t={}; t:'..name..'()')
 check('method bad self',name,'local t={}; t.'..name..'=string.sub; t:'..name..'(false)')
 check('method argument',name,'getmetatable("").__index.'..name..'=string.sub; ("abc"):'..name..'(false)')
end
for byte=0,127 do
 if byte<32 or byte==127 then
  local name='n'..string.char(byte)..'z'
  print('identifier boundary',byte,
   loadstring('local '..name..'=nil')~=nil,
   loadstring(name..'=nil')~=nil,
   loadstring('local t={}; t:'..name..'()')~=nil)
 end
end
-- Whitespace can separate statements (local n; z=nil); it does not join the identifier.
