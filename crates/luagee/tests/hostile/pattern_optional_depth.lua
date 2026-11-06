-- expect: error pattern too complex
return string.find(("a"):rep(500), ("a?"):rep(500) .. "b")
