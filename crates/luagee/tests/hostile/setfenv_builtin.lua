-- expect: error environment
return setfenv(print, {})
