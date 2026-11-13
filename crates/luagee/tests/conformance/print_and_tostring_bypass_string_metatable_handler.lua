local meta=getmetatable('')
local calls=0
meta.__tostring=function() calls=calls+1; return 'CHANGED' end
print(tostring('x'),'x',string.format('%s','z'),string.format('%q','q'))
print('', 'a\0b', tostring(''), tostring('a\0b'))
print('handler calls',calls)
meta.__tostring=function() error('string handler must not run') end
print('unchanged',tostring('unchanged'),string.format('%s','unchanged'))
meta.__tostring=nil
