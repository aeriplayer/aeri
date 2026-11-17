//! Reading `ReplayGain` values out of tag text (ADR 0022).

/// A gain in decibels: a decimal number with an optional `dB` suffix in any case.
pub(crate) fn parse_gain(text: &str) -> Option<f32> {
    let text = text.trim();
    let number = match text
        .len()
        .checked_sub(2)
        .and_then(|at| text.split_at_checked(at))
    {
        Some((number, suffix)) if suffix.eq_ignore_ascii_case("db") => number,
        _ => text,
    };
    number
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|gain| gain.is_finite())
}

/// A peak: a finite, non-negative decimal number.
pub(crate) fn parse_peak(text: &str) -> Option<f32> {
    text.trim()
        .parse::<f32>()
        .ok()
        .filter(|peak| peak.is_finite() && *peak >= 0.0)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn gains_accept_the_common_spellings() {
        for (text, expected) in [
            ("-6.54 dB", -6.54),
            ("-6.54dB", -6.54),
            ("-6.54 DB", -6.54),
            ("-6.54 db", -6.54),
            ("+3.2 dB", 3.2),
            ("  0.00 dB  ", 0.0),
            ("-6.54", -6.54),
            ("12", 12.0),
        ] {
            assert_eq!(parse_gain(text), Some(expected), "{text:?}");
        }
    }

    #[test]
    fn gains_reject_what_is_not_a_finite_number() {
        for text in [
            "", "dB", "loud", "NaN dB", "inf", "-inf dB", "1,5 dB", "6 dB dB", "é", "é dB",
        ] {
            assert_eq!(parse_gain(text), None, "{text:?}");
        }
    }

    #[test]
    fn peaks_accept_non_negative_decimals() {
        assert_eq!(parse_peak("0.088000"), Some(0.088));
        assert_eq!(parse_peak(" 1.0 "), Some(1.0));
        assert_eq!(parse_peak("0"), Some(0.0));
        assert_eq!(parse_peak("1.2"), Some(1.2));
    }

    #[test]
    fn peaks_reject_negative_and_non_finite_values() {
        for text in ["", "loud", "-0.5", "NaN", "inf", "0.5 dB"] {
            assert_eq!(parse_peak(text), None, "{text:?}");
        }
    }

    proptest! {
        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn no_text_makes_the_parsers_panic_or_return_a_non_finite_value(text in any::<String>()) {
            prop_assert!(parse_gain(&text).is_none_or(f32::is_finite));
            prop_assert!(parse_peak(&text).is_none_or(|peak| peak.is_finite() && peak >= 0.0));
        }

        #[test]
        #[cfg_attr(miri, ignore = "proptest reads its regression file, which Miri's isolation forbids")]
        fn a_written_gain_reads_back_with_or_without_the_suffix(gain in -100.0_f32..100.0) {
            prop_assert_eq!(parse_gain(&format!("{gain} dB")), Some(gain));
            prop_assert_eq!(parse_gain(&format!("{gain}")), Some(gain));
        }
    }
}
