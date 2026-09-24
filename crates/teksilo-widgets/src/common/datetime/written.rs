// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Dates written for a person to read or hear, in the widget's locale.
//!
//! The date widgets show the value being edited through a pattern
//! ([`pattern`](super::pattern)), because that text is typed back and has to
//! parse. Everything else they say about a date is only read: an
//! accessibility name or value, a calendar's title, a range summary. That goes
//! through ICU here and nowhere else. A `format!` of numbers gives a French
//! user "2026-09-24"; a concatenation of translated weekday and month names
//! gives the source language's word order and the nominative where Russian or
//! Polish need the genitive. ICU chooses the pattern for the whole date, so
//! both come out as the locale writes them.
//!
//! Every date is counted on the Gregorian calendar, whichever calendar the
//! locale prefers, because that is the calendar the widgets lay their days
//! out on (see [`gregorian`]). One thing ICU does not do is done here:
//! French, Italian and Romanian name the first of the month as an ordinal
//! (see [`first_of_month`]).

use teksilo_i18n::{
    CalendarSystem, DateFields, DateStyle, LanguageIdentifier, TeksiloDateTimeFormatter, TimeStyle,
};

use super::types::{Date, DateTime, Time, YearMonth};

/// The locale a date widget writes its dates in.
///
/// The widget tree's locale comes first: the same widgets already take their
/// pattern and their first day of week from it, and the app layer keeps it
/// equal to the `I18nManager`'s. The manager's comes next, for a tree nobody
/// gave a locale. Last is en-US, the framework's source language, because the
/// root locale ICU would otherwise use writes "2026 M08 31, Mon".
pub(crate) fn date_locale(tree_locale: Option<&str>) -> LanguageIdentifier {
    tree_locale
        .filter(|tag| !tag.is_empty())
        .and_then(|tag| tag.parse().ok())
        .or_else(|| teksilo_i18n::current_locale().map(|locale| locale.get()))
        // A literal that parses; `unwrap_or_default` only keeps a panic out
        // of the path.
        .unwrap_or_else(|| "en-US".parse().unwrap_or_default())
}

/// The formatter every date here starts from, on the Gregorian calendar.
///
/// ICU writes a date in the calendar the locale prefers, and CLDR prefers the
/// Persian one for `fa-IR` and the Buddhist era for `th-TH`. The month grid,
/// the day numbers drawn in it and the fields' patterns are Gregorian in
/// every locale, so a Persian title for March 2027 would name Esfand 1405
/// above days most of which are not Esfand's, and each cell would name a
/// different day number from the one drawn in it.
fn gregorian() -> TeksiloDateTimeFormatter {
    TeksiloDateTimeFormatter::new().calendar_system(CalendarSystem::Gregorian)
}

/// How a language names the first of the month where CLDR writes a bare "1".
///
/// `spoken` goes into a date written for the ear, `written` into one drawn on
/// screen; `None` there keeps the digit, because the language writes it.
struct FirstOfMonth {
    language: &'static str,
    spoken: &'static str,
    written: Option<&'static str>,
}

/// Every language whose own convention names the first of the month as an
/// ordinal while CLDR's date patterns write "1", and names every other day
/// by its number. A speech engine reads the "1" as the cardinal: espeak-ng,
/// which speech-dispatcher gives Orca by default and which NVDA ships, says
/// "mardi un septembre", "lunedì uno marzo", "luni unu martie".
///
/// - French: "le premier septembre", written "1er".
/// - Italian: "il primo marzo", with every later day a cardinal (Serianni,
///   cited by the Accademia della Crusca: "si usa l'ordinale per il giorno
///   iniziale [...], ma il cardinale per i giorni successivi"). Treccani
///   writes it with the sign ‹°› ("il 1° aprile"); this writes the ordinal
///   indicator "1º", the character that sign stands for, which espeak-ng
///   reads "primo" where it reads "1°" as "uno gradi".
/// - Romanian: "Întâi Decembrie", not "Unu Decembrie" (DOOM2: "pentru
///   indicarea primei zile a fiecărei luni trebuie folosit numeralul
///   ordinal"). Romanian writes the digit and reads it "întâi", so only the
///   spoken date changes.
///
/// Not here, each on purpose. Spanish in Spain and European Portuguese say
/// "uno de septiembre" and "um de março" as readily as the ordinal (the
/// RAE; Ciberdúvidas), so ICU's digit already reads right. Where a language
/// marks the ordinal itself (German "1. März", Danish, Norwegian, Finnish,
/// Czech, Hungarian) CLDR writes the marker. Russian, Ukrainian, Polish and
/// Swedish read every day of the month as an ordinal, and Greek does in its
/// formal register ("από 1η Μαΐου έως 31η Οκτωβρίου"), which a rule for the
/// first alone would get wrong on the second; that is a question for every
/// day, left open. Dutch and Turkish name every day by its number. Arabic and
/// Hebrew were not settled and are left as ICU writes them.
const FIRST_OF_MONTH: [FirstOfMonth; 3] = [
    FirstOfMonth {
        language: "fr",
        spoken: "premier",
        written: Some("1er"),
    },
    FirstOfMonth {
        language: "it",
        spoken: "primo",
        written: Some("1º"),
    },
    FirstOfMonth {
        language: "ro",
        spoken: "întâi",
        written: None,
    },
];

/// Whether a date is being written for the ear or drawn on screen.
#[derive(Clone, Copy)]
enum Medium {
    Spoken,
    Written,
}

/// The first of the month the way the language names it, in a date ICU
/// wrote, for the languages in [`FIRST_OF_MONTH`]. Every other day is its
/// number, and every other language is left as ICU writes it.
///
/// The day is the one token that is exactly "1" in these languages' dates:
/// the year has four digits and their clocks pad the hour ("09:05").
fn first_of_month(written: String, day: i8, lang: &LanguageIdentifier, medium: Medium) -> String {
    if day != 1 {
        return written;
    }
    let Some(rule) = FIRST_OF_MONTH
        .iter()
        .find(|rule| rule.language == lang.language.as_str())
    else {
        return written;
    };
    let ordinal = match medium {
        Medium::Spoken => rule.spoken,
        Medium::Written => match rule.written {
            Some(ordinal) => ordinal,
            None => return written,
        },
    };
    let mut said = String::with_capacity(written.len() + ordinal.len());
    let mut replaced = false;
    for piece in written.split_inclusive(char::is_whitespace) {
        let token = piece.trim_end_matches(char::is_whitespace);
        if !replaced && token == "1" {
            said.push_str(ordinal);
            said.push_str(&piece[token.len()..]);
            replaced = true;
        } else {
            said.push_str(piece);
        }
    }
    said
}

/// A day in full, with its weekday: "lundi 31 août 2026", "Monday, August 31,
/// 2026", "mardi premier septembre 2026". How a day is named when it is
/// spoken.
pub(crate) fn full_date(date: Date, lang: &LanguageIdentifier) -> String {
    let written = gregorian()
        .date_style(DateStyle::Long)
        .date_fields(DateFields::YearMonthDayWeekday)
        .format_in_locale(date.to_datetime(Time::midnight()), lang);
    first_of_month(written, date.day(), lang, Medium::Spoken)
}

/// A month with its year: "septembre 2026", "September 2026".
pub(crate) fn month_and_year(month: YearMonth, lang: &LanguageIdentifier) -> String {
    gregorian()
        .date_style(DateStyle::Long)
        .date_fields(DateFields::YearMonth)
        .format_in_locale(month.first_day().to_datetime(Time::midnight()), lang)
}

/// A day, abbreviated where the locale abbreviates: "1er sept. 2026", "Sep 1,
/// 2026". For text on screen where a full date would not fit.
pub(crate) fn medium_date(date: Date, lang: &LanguageIdentifier) -> String {
    let written = gregorian()
        .date_style(DateStyle::Medium)
        .format_in_locale(date.to_datetime(Time::midnight()), lang);
    first_of_month(written, date.day(), lang, Medium::Written)
}

/// A day in full with a time of day: "samedi 2 mai 2026 à 14:35", "Saturday,
/// May 2, 2026 at 2:35 PM". The seconds are written only when `seconds` is
/// set, so a field that hides them does not speak them.
pub(crate) fn full_date_time(value: DateTime, seconds: bool, lang: &LanguageIdentifier) -> String {
    let written = gregorian()
        .date_style(DateStyle::Long)
        .date_fields(DateFields::YearMonthDayWeekday)
        .time_style(if seconds {
            TimeStyle::Medium
        } else {
            TimeStyle::Short
        })
        .format_in_locale(value, lang);
    first_of_month(written, value.day(), lang, Medium::Spoken)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang(tag: &str) -> LanguageIdentifier {
        tag.parse().expect("test tag parses")
    }

    #[test]
    fn a_day_is_written_in_full_in_the_locales_own_order() {
        let day = Date::constant(2026, 8, 31);
        assert_eq!(full_date(day, &lang("fr-FR")), "lundi 31 août 2026");
        assert_eq!(full_date(day, &lang("en-US")), "Monday, August 31, 2026");
    }

    #[test]
    fn a_month_is_written_with_its_year_and_no_day() {
        let month = YearMonth::new(2026, 9);
        assert_eq!(month_and_year(month, &lang("fr-FR")), "septembre 2026");
        assert_eq!(month_and_year(month, &lang("en-US")), "September 2026");
    }

    #[test]
    fn a_time_is_written_after_the_day_with_seconds_only_when_asked() {
        let value = Date::constant(2026, 5, 2).at(14, 35, 7, 0);
        assert_eq!(
            full_date_time(value, false, &lang("fr-FR")),
            "samedi 2 mai 2026 à 14:35"
        );
        assert_eq!(
            full_date_time(value, true, &lang("fr-FR")),
            "samedi 2 mai 2026 à 14:35:07"
        );
    }

    #[test]
    fn every_date_stays_on_the_gregorian_calendar_the_widgets_lay_out() {
        // fa-IR prefers the Persian calendar and th-TH the Buddhist era.
        // The grid is Gregorian in both, so its title and its days are too:
        // March 2027 rather than Esfand 1405, and 2027 rather than 2570.
        let fa = lang("fa-IR");
        assert_eq!(month_and_year(YearMonth::new(2027, 3), &fa), "مارس ۲۰۲۷");
        assert_eq!(
            full_date(Date::constant(2027, 3, 12), &fa),
            "جمعه ۱۲ مارس ۲۰۲۷"
        );
        assert_eq!(medium_date(Date::constant(2027, 3, 1), &fa), "۱ مارس ۲۰۲۷");
        let th = lang("th-TH");
        assert_eq!(
            month_and_year(YearMonth::new(2027, 3), &th),
            "มีนาคม ค.ศ. 2027"
        );
        let with_time = full_date_time(Date::constant(2027, 3, 12).at(9, 5, 0, 0), false, &th);
        assert!(with_time.contains("ค.ศ. 2027"), "{with_time}");
    }

    #[test]
    fn french_says_the_first_of_the_month_as_an_ordinal() {
        // A speech engine reads a bare "1" as "un": "mardi un septembre".
        // Spoken, the first is "premier"; on screen it is written "1er".
        let fr = lang("fr-FR");
        let first = Date::constant(2026, 9, 1);
        assert_eq!(full_date(first, &fr), "mardi premier septembre 2026");
        assert_eq!(medium_date(first, &fr), "1er sept. 2026");
        assert_eq!(
            full_date_time(first.at(1, 5, 0, 0), false, &fr),
            "mardi premier septembre 2026 à 01:05"
        );
        // Only the first of the month: the 11th and the 21st keep their
        // numbers, and so does a year with a 1 in it.
        assert_eq!(
            full_date(Date::constant(2001, 1, 11), &fr),
            "jeudi 11 janvier 2001"
        );
        assert_eq!(
            full_date(Date::constant(2001, 1, 21), &fr),
            "dimanche 21 janvier 2001"
        );
        // Only French: Spanish says "uno de septiembre" from a bare 1, and
        // English says "first" from "September 1".
        assert_eq!(
            full_date(first, &lang("es-ES")),
            "martes, 1 de septiembre de 2026"
        );
        assert_eq!(
            full_date(first, &lang("en-US")),
            "Tuesday, September 1, 2026"
        );
    }

    #[test]
    fn italian_and_romanian_say_the_first_of_the_month_as_an_ordinal() {
        // CLDR writes "lunedì 1 marzo" and "luni, 1 martie", which espeak-ng
        // reads "uno marzo" and "unu martie". Italian says "primo" and writes
        // "1º"; Romanian says "întâi" and writes the digit.
        let first = Date::constant(2027, 3, 1);
        let it = lang("it-IT");
        assert_eq!(full_date(first, &it), "lunedì primo marzo 2027");
        assert_eq!(medium_date(first, &it), "1º mar 2027");
        assert_eq!(
            full_date_time(first.at(1, 5, 0, 0), false, &it),
            "lunedì primo marzo 2027 alle ore 01:05"
        );
        assert_eq!(
            full_date(Date::constant(2027, 3, 11), &it),
            "giovedì 11 marzo 2027"
        );
        assert_eq!(
            full_date(Date::constant(2027, 3, 21), &it),
            "domenica 21 marzo 2027"
        );
        let ro = lang("ro-RO");
        assert_eq!(full_date(first, &ro), "luni, întâi martie 2027");
        assert_eq!(medium_date(first, &ro), "1 mar. 2027");
        assert_eq!(
            full_date_time(first.at(1, 5, 0, 0), false, &ro),
            "luni, întâi martie 2027 la 01:05"
        );
        assert_eq!(
            full_date(Date::constant(2027, 3, 11), &ro),
            "joi, 11 martie 2027"
        );
    }

    #[test]
    fn a_language_that_reads_its_first_right_keeps_it_as_icu_writes_it() {
        // Spanish in Spain says "uno de marzo" as readily as "primero", the
        // languages that mark the ordinal get the mark from CLDR, and those
        // that read every day as an ordinal are not a question about the
        // first. None of them is rewritten.
        let first = Date::constant(2027, 3, 1);
        for (tag, said) in [
            ("es-ES", "lunes, 1 de marzo de 2027"),
            ("de-DE", "Montag, 1. März 2027"),
            ("el-GR", "Δευτέρα 1 Μαρτίου 2027"),
            ("ru-RU", "понедельник, 1 марта 2027\u{202f}г."),
            ("sv-SE", "måndag 1 mars 2027"),
            ("nl-NL", "maandag 1 maart 2027"),
        ] {
            assert_eq!(full_date(first, &lang(tag)), said, "{tag}");
        }
    }

    #[test]
    fn the_tree_locale_wins_and_nothing_at_all_falls_back_to_english() {
        teksilo_i18n::thread_local::clear();
        assert_eq!(date_locale(Some("fr-FR")), lang("fr-FR"));
        assert_eq!(date_locale(Some("")), lang("en-US"));
        assert_eq!(date_locale(None), lang("en-US"));
    }
}
