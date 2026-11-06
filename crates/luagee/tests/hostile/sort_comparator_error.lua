-- expect: error cmp
table.sort({3, 2, 1}, function() error("cmp") end)
