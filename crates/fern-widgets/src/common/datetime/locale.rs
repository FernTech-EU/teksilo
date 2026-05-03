//! Per-locale defaults: first day of week + segment-pattern.
//!
//! Hand-curated for the top ~25 locales, matching CLDR. Any locale not
//! listed falls back to ISO 8601: Monday first day, `YYYY-MM-DD`. Both
//! defaults are overridable on the widget builder.

use jiff::civil::Weekday;

/// Default first day of week for the given locale tag (e.g. `"en-US"`,
/// `"fr-FR"`). Lookup is a longest-prefix match: a fully-qualified
/// `"en-US"` resolves before the bare `"en"` family.
pub fn first_day_of_week_for_locale(tag: &str) -> Weekday {
    let normalized = tag.to_ascii_lowercase();

    // Region-specific overrides first.
    for &(prefix, weekday) in REGION_FIRST_DAY {
        if normalized == prefix || normalized.starts_with(&format!("{prefix}-")) {
            return weekday;
        }
    }
    // Then language-only fallback.
    for &(prefix, weekday) in LANG_FIRST_DAY {
        if normalized == prefix || normalized.starts_with(&format!("{prefix}-")) {
            return weekday;
        }
    }
    // ISO 8601 default.
    Weekday::Monday
}

/// Default date format pattern for the given locale tag. Uses the
/// strftime subset described in `pattern.rs`.
pub fn format_pattern_for_locale(tag: &str) -> &'static str {
    let normalized = tag.to_ascii_lowercase();

    for &(prefix, pat) in REGION_PATTERNS {
        if normalized == prefix || normalized.starts_with(&format!("{prefix}-")) {
            return pat;
        }
    }
    for &(prefix, pat) in LANG_PATTERNS {
        if normalized == prefix || normalized.starts_with(&format!("{prefix}-")) {
            return pat;
        }
    }
    // ISO 8601 fallback.
    "%Y-%m-%d"
}

// ──────────────────────────────────────────────────────────────────
// Tables. Region-specific entries take precedence over language-only
// fallbacks so `en-US` resolves before the generic `en`. Lookup
// strings must be lowercase and match the *prefix* of a normalized
// locale tag.

/// Per-region first-day-of-week. Ordered most-specific first; the
/// resolver short-circuits on the first match.
const REGION_FIRST_DAY: &[(&str, Weekday)] = &[
    ("en-us", Weekday::Sunday),
    ("en-ca", Weekday::Sunday),
    ("en-au", Weekday::Sunday),
    ("ja-jp", Weekday::Sunday),
    ("zh-cn", Weekday::Sunday),
    ("zh-tw", Weekday::Sunday),
    ("ko-kr", Weekday::Sunday),
    ("he-il", Weekday::Sunday),
    ("ar-eg", Weekday::Saturday),
    ("ar-sa", Weekday::Sunday),
    ("ar-ae", Weekday::Saturday),
    ("ar-dz", Weekday::Saturday),
    ("ar-ma", Weekday::Saturday),
    ("fa-ir", Weekday::Saturday),
];

/// Per-language first-day-of-week. Used when no region match was found.
const LANG_FIRST_DAY: &[(&str, Weekday)] = &[
    // Most of Western and Eastern Europe + global "Monday-first" group.
    ("en", Weekday::Monday),
    ("fr", Weekday::Monday),
    ("de", Weekday::Monday),
    ("es", Weekday::Monday),
    ("it", Weekday::Monday),
    ("pt", Weekday::Monday),
    ("nl", Weekday::Monday),
    ("sv", Weekday::Monday),
    ("nb", Weekday::Monday),
    ("nn", Weekday::Monday),
    ("da", Weekday::Monday),
    ("fi", Weekday::Monday),
    ("pl", Weekday::Monday),
    ("ru", Weekday::Monday),
    ("uk", Weekday::Monday),
    ("cs", Weekday::Monday),
    ("sk", Weekday::Monday),
    ("hu", Weekday::Monday),
    ("ro", Weekday::Monday),
    ("bg", Weekday::Monday),
    ("hr", Weekday::Monday),
    ("sr", Weekday::Monday),
    ("sl", Weekday::Monday),
    ("el", Weekday::Monday),
    ("tr", Weekday::Monday),
];

/// Per-region default pattern. Ordered most-specific first.
const REGION_PATTERNS: &[(&str, &str)] = &[
    ("en-us", "%m/%d/%Y"),
    ("en-ca", "%Y-%m-%d"),
    ("en-gb", "%d/%m/%Y"),
    ("en-au", "%d/%m/%Y"),
    ("en-ie", "%d/%m/%Y"),
    ("en-nz", "%d/%m/%Y"),
    ("ja-jp", "%Y/%m/%d"),
    ("zh-cn", "%Y/%m/%d"),
    ("zh-tw", "%Y/%m/%d"),
    ("ko-kr", "%Y. %m. %d."),
    ("de-de", "%d.%m.%Y"),
    ("de-at", "%d.%m.%Y"),
    ("de-ch", "%d.%m.%Y"),
    ("fr-ca", "%Y-%m-%d"),
];

/// Per-language default pattern. Used when no region match was found.
const LANG_PATTERNS: &[(&str, &str)] = &[
    ("en", "%Y-%m-%d"),
    ("fr", "%d/%m/%Y"),
    ("es", "%d/%m/%Y"),
    ("it", "%d/%m/%Y"),
    ("pt", "%d/%m/%Y"),
    ("nl", "%d-%m-%Y"),
    ("de", "%d.%m.%Y"),
    ("sv", "%Y-%m-%d"),
    ("nb", "%d.%m.%Y"),
    ("nn", "%d.%m.%Y"),
    ("da", "%d-%m-%Y"),
    ("fi", "%d.%m.%Y"),
    ("pl", "%d.%m.%Y"),
    ("ru", "%d.%m.%Y"),
    ("uk", "%d.%m.%Y"),
    ("cs", "%d.%m.%Y"),
    ("sk", "%d.%m.%Y"),
    ("hu", "%Y. %m. %d."),
    ("ro", "%d.%m.%Y"),
    ("bg", "%d.%m.%Y"),
    ("hr", "%d.%m.%Y"),
    ("sr", "%d.%m.%Y"),
    ("sl", "%d.%m.%Y"),
    ("el", "%d/%m/%Y"),
    ("tr", "%d.%m.%Y"),
    ("ja", "%Y/%m/%d"),
    ("zh", "%Y/%m/%d"),
    ("ko", "%Y. %m. %d."),
    ("ar", "%d/%m/%Y"),
    ("he", "%d/%m/%Y"),
    ("fa", "%Y/%m/%d"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_day_us_is_sunday_eu_is_monday() {
        assert_eq!(first_day_of_week_for_locale("en-US"), Weekday::Sunday);
        assert_eq!(first_day_of_week_for_locale("fr-FR"), Weekday::Monday);
        assert_eq!(first_day_of_week_for_locale("de-DE"), Weekday::Monday);
        assert_eq!(first_day_of_week_for_locale("ja-JP"), Weekday::Sunday);
    }

    #[test]
    fn first_day_unknown_locale_falls_back_to_iso() {
        assert_eq!(first_day_of_week_for_locale("xx-XX"), Weekday::Monday);
    }

    #[test]
    fn pattern_us_is_mdy_eu_is_dmy() {
        assert_eq!(format_pattern_for_locale("en-US"), "%m/%d/%Y");
        assert_eq!(format_pattern_for_locale("en-GB"), "%d/%m/%Y");
        assert_eq!(format_pattern_for_locale("fr-FR"), "%d/%m/%Y");
        assert_eq!(format_pattern_for_locale("de-DE"), "%d.%m.%Y");
        assert_eq!(format_pattern_for_locale("ja-JP"), "%Y/%m/%d");
        assert_eq!(format_pattern_for_locale("sv-SE"), "%Y-%m-%d");
    }

    #[test]
    fn region_overrides_language() {
        // `en` defaults to ISO; `en-US` overrides to MDY.
        assert_eq!(format_pattern_for_locale("en"), "%Y-%m-%d");
        assert_eq!(format_pattern_for_locale("en-US"), "%m/%d/%Y");
    }
}
