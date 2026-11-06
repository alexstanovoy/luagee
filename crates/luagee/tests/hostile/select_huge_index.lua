-- expect: ok 0	0
print(select("#", select(100, 1, 2)), select("#", select(2^30, 1, 2)))
