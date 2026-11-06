-- expect: error capture
return string.find(("a"):rep(500), ("(a)"):rep(500))
