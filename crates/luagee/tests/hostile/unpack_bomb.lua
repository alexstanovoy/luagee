-- expect: error too many
return select("#", unpack({}, 1, 1e8))
