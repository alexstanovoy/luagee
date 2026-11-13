-- expect: limit fuel_per_call
-- These endpoints each advance, but the control between them cannot advance by one.
table.move({},2^53+2,2^53+6,1)
