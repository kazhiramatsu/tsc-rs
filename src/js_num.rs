//! JavaScript Number#toString semantics (ECMA-262 §7.1.12.1) — needed so
//! numeric literal types display byte-identically to tsc (e.g. `1e+21`,
//! `1e-7`, `-0` → `0`).

pub fn to_js_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string(); // covers -0
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let neg = x < 0.0;
    let x = x.abs();

    // Rust {:e} gives the shortest round-trip representation in exponential form.
    let sci = format!("{:e}", x); // e.g. "1.5e0", "1e21", "9.99e-7"
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let n = digits.len() as i32; // number of significant digits
    let k = exp + 1; // decimal point position: value = 0.digits * 10^k

    let body = if n <= k && k <= 21 {
        // integer with trailing zeros
        let mut s = digits.to_string();
        for _ in 0..(k - n) {
            s.push('0');
        }
        s
    } else if 0 < k && k <= 21 {
        // point inside digits
        let (a, b) = digits.split_at(k as usize);
        format!("{a}.{b}")
    } else if -6 < k && k <= 0 {
        let mut s = String::from("0.");
        for _ in 0..-k {
            s.push('0');
        }
        s.push_str(digits);
        s
    } else {
        // exponential
        let e = k - 1;
        let sign = if e >= 0 { "+" } else { "-" };
        let mant = if n == 1 {
            digits.to_string()
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{mant}e{sign}{}", e.abs())
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::to_js_string as s;

    #[test]
    fn js_number_formatting() {
        assert_eq!(s(1.5), "1.5");
        assert_eq!(s(100.0), "100");
        assert_eq!(s(-0.0), "0");
        assert_eq!(s(0.5), "0.5");
        assert_eq!(s(1e21), "1e+21");
        assert_eq!(s(1e-7), "1e-7");
        assert_eq!(s(0.0000001), "1e-7");
        assert_eq!(s(1e-6), "0.000001");
        assert_eq!(s(1e20), "100000000000000000000");
        assert_eq!(s(123.456), "123.456");
        assert_eq!(s(-42.0), "-42");
        assert_eq!(s(2.5e21), "2.5e+21");
        assert_eq!(s(f64::NAN), "NaN");
        assert_eq!(s(0.1 + 0.2), "0.30000000000000004");
    }
}
