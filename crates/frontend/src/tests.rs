use super::*;

#[test]
fn civil_day_roundtrip() {
    for iso in ["2026-06-11", "2024-02-29", "1999-12-31", "2026-01-01"] {
        let n = iso_day_number(iso).expect("parses");
        let (y, m, d) = civil_from_days(n);
        assert_eq!(format!("{y:04}-{m:02}-{d:02}"), iso);
    }
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(
        days_from_civil(2026, 6, 12) - days_from_civil(2026, 6, 11),
        1
    );
}

#[test]
fn month_lengths() {
    assert_eq!(days_in_month(2026, 6), 30);
    assert_eq!(days_in_month(2026, 7), 31);
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2026, 2), 28);
    assert_eq!(days_in_month(2000, 2), 29);
    assert_eq!(days_in_month(1900, 2), 28);
}

#[test]
fn calendar_months_align_to_monday_first_grid() {
    assert_eq!(calendar_month_offset(2026, 6), 0);
    assert_eq!(calendar_month_offset(2026, 7), 2);
    assert_eq!(calendar_weekday_index(days_from_civil(2026, 6, 1)), 0);
    assert_eq!(calendar_weekday_index(days_from_civil(2026, 6, 7)), 6);
}

#[test]
fn calendar_month_navigation_wraps_at_year_boundaries() {
    assert_eq!(prev_month(2026, 6), (2026, 5));
    assert_eq!(next_month(2026, 6), (2026, 7));
    assert_eq!(prev_month(2026, 1), (2025, 12));
    assert_eq!(next_month(2026, 12), (2027, 1));
}

#[test]
fn calendar_weekends_are_saturday_and_sunday() {
    assert!(!calendar_is_weekend(days_from_civil(2026, 6, 5)));
    assert!(calendar_is_weekend(days_from_civil(2026, 6, 6)));
    assert!(calendar_is_weekend(days_from_civil(2026, 6, 7)));
    assert!(!calendar_is_weekend(days_from_civil(2026, 6, 8)));
}

#[test]
fn gantt_weekends_match_calendar_weekends() {
    assert!(!is_weekend(days_from_civil(2026, 6, 5)));
    assert!(is_weekend(days_from_civil(2026, 6, 6)));
    assert!(is_weekend(days_from_civil(2026, 6, 7)));
    assert!(!is_weekend(days_from_civil(2026, 6, 8)));
}

#[test]
fn dates_format_with_real_month_names() {
    assert_eq!(fmt_date("2026-03-05", Lang::De), "5. Mär");
    assert_eq!(fmt_date("2026-03-05", Lang::En), "Mar 5");
    assert_eq!(fmt_date("2026-12-24", Lang::De), "24. Dez");
    assert_eq!(fmt_date("not-a-date", Lang::De), "not-a-date");
}

#[test]
fn gantt_dates_include_partial_task_dates() {
    let june_10 = days_from_civil(2026, 6, 10);
    let june_12 = days_from_civil(2026, 6, 12);

    assert_eq!(
        scheduled_task_days(Some("2026-06-10"), Some("2026-06-12")),
        Some((june_10, june_12))
    );
    assert_eq!(
        scheduled_task_days(None, Some("2026-06-12")),
        Some((june_12, june_12))
    );
    assert_eq!(
        scheduled_task_days(Some("2026-06-10"), None),
        Some((june_10, june_10))
    );
}

#[test]
fn gantt_dates_ignore_invalid_unscheduled_tasks() {
    assert_eq!(scheduled_task_days(None, None), None);
    assert_eq!(scheduled_task_days(Some("not-a-date"), None), None);
    assert_eq!(scheduled_task_days(None, Some("2026-99-99")), None);
}

#[test]
fn gantt_dates_normalize_reversed_ranges() {
    assert_eq!(
        scheduled_task_days(Some("2026-06-12"), Some("2026-06-10")),
        Some((days_from_civil(2026, 6, 10), days_from_civil(2026, 6, 12)))
    );
}

#[test]
fn gantt_bounds_include_milestones() {
    let task_start = days_from_civil(2026, 6, 10);
    let task_due = days_from_civil(2026, 6, 12);
    let milestone = days_from_civil(2026, 6, 18);

    assert_eq!(
        timeline_bounds([(task_start, task_due)], [milestone]),
        Some((task_start, milestone))
    );
    assert_eq!(
        timeline_bounds([], [milestone]),
        Some((milestone, milestone))
    );
    assert_eq!(
        timeline_bounds(std::iter::empty::<(i64, i64)>(), std::iter::empty::<i64>()),
        None
    );
}

#[test]
fn gantt_month_segments_split_cross_month_ranges() {
    let start = days_from_civil(2026, 6, 29);

    assert_eq!(
        gantt_month_segments(start, 4),
        vec![
            GanttMonthSegment {
                year: 2026,
                month: 6,
                days: 2,
            },
            GanttMonthSegment {
                year: 2026,
                month: 7,
                days: 2,
            },
        ]
    );
}

#[test]
fn mention_query_finds_trailing_fragment() {
    assert_eq!(mention_query("hallo @An"), Some((6, "An".to_string())));
    assert_eq!(mention_query("@"), Some((0, String::new())));
    assert_eq!(
        mention_query("@Anna Sch"),
        Some((0, "Anna Sch".to_string()))
    );
    // '@' glued to a word (e-mail address) is not a mention trigger.
    assert_eq!(mention_query("mail an anna@web.de"), None);
    assert_eq!(mention_query("kein at"), None);
}

#[test]
fn mention_segments_highlight_known_names() {
    let names = vec!["Anna".to_string(), "Anna Schmidt".to_string()];
    assert_eq!(
        mention_segments("ping @Anna Schmidt jetzt", &names),
        vec![
            ("ping ".to_string(), false),
            ("@Anna Schmidt".to_string(), true),
            (" jetzt".to_string(), false),
        ]
    );
    // Boundary: longer words never match a shorter member name.
    assert_eq!(
        mention_segments("@Annabelle hi", &names),
        vec![("@Annabelle hi".to_string(), false)]
    );
    assert_eq!(
        mention_segments("ohne mention", &names),
        vec![("ohne mention".to_string(), false)]
    );
}

#[test]
fn attachment_extensions_are_lowercased() {
    assert_eq!(attachment_ext("Plan.PDF"), "pdf");
    assert_eq!(attachment_ext("foto.jpeg"), "jpeg");
    assert_eq!(attachment_ext("noext"), "");
}
