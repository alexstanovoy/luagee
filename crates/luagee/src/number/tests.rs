use super::{format, modulo};

/// What LuaJIT 2.1 prints for each number (`tostring`), the dialect's reference.
const PRINTED: [(f64, &str); 31] = [
    (0.0, "0"),
    (-0.0, "-0"),
    (1.0, "1"),
    (-1.0, "-1"),
    (0.1, "0.1"),
    (1.0 / 3.0, "0.33333333333333"),
    (-2.0 / 3.0, "-0.66666666666667"),
    (100.0, "100"),
    (1e14, "1e+14"),
    (1e15, "1e+15"),
    (123_456_789_012_345.0, "1.2345678901234e+14"),
    (1_234_567_890_123_456.0, "1.2345678901235e+15"),
    (99_999_999_999_999.5, "1e+14"),
    (0.0001, "0.0001"),
    (0.00001, "1e-05"),
    (1.5e-7, "1.5e-07"),
    (9_007_199_254_740_992.0, "9.007199254741e+15"),
    (9_223_372_036_854_775_808.0, "9.2233720368548e+18"),
    (-9_223_372_036_854_775_808.0, "-9.2233720368548e+18"),
    (1e100, "1e+100"),
    (f64::INFINITY, "inf"),
    (f64::NEG_INFINITY, "-inf"),
    (5e-324, "4.9406564584125e-324"),
    (f64::MAX, "1.7976931348623e+308"),
    (7.123456789012345, "7.1234567890123"),
    (9.99999999999995, "10"),
    (0.30000000000000004, "0.3"),
    (1e21, "1e+21"),
    (0.99609375, "0.99609375"),
    (12.5, "12.5"),
    (9.999999999999949e-5, "9.9999999999999e-05"),
];

#[test]
fn numbers_print_as_luajit_prints_them() {
    for (number, text) in PRINTED {
        assert_eq!(format(number), text, "{number:e}");
    }
}

#[test]
fn every_nan_prints_the_same() {
    for bits in [
        0x7ff8_0000_0000_0000_u64,
        0xfff8_0000_0000_0000,
        0x7ff0_0000_0000_0001,
        0xffff_ffff_ffff_ffff,
    ] {
        assert_eq!(format(f64::from_bits(bits)), "nan", "{bits:#x}");
    }
}

#[test]
fn modulo_takes_the_divisors_sign_as_lua_5_1_defines_it() {
    for (dividend, divisor, remainder) in [
        (5.5, 2.0, "1.5"),
        (-5.0, 3.0, "1"),
        (5.0, -3.0, "-1"),
        (5.3, 1.0, "0.3"),
        (-0.5, 1.0, "0.5"),
        (1e308, 3.0, "0"),
    ] {
        assert_eq!(
            format(modulo(dividend, divisor)),
            remainder,
            "{dividend} % {divisor}"
        );
    }
}

#[test]
fn modulo_by_zero_or_infinity_is_nan() {
    for (dividend, divisor) in [
        (5.0, 0.0),
        (-5.0, 0.0),
        (f64::INFINITY, 2.0),
        (2.0, f64::INFINITY),
        (-2.0, f64::INFINITY),
        (f64::NAN, 1.0),
    ] {
        assert!(modulo(dividend, divisor).is_nan(), "{dividend} % {divisor}");
    }
}
