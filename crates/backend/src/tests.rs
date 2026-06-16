use super::*;

#[test]
fn initials_use_first_two_words() {
    assert_eq!(initials("Alex Lindner"), "AL");
    assert_eq!(initials("Mira"), "M");
}

#[test]
fn size_labels_are_human_readable() {
    assert_eq!(size_label(18_000), "18 KB");
    assert_eq!(size_label(1_250_000), "1.2 MB");
}

#[test]
fn magic_numbers_gate_inline_previewable_types() {
    assert!(magic_matches("doc.pdf", b"%PDF-1.7 rest"));
    assert!(!magic_matches("doc.pdf", b"MZ\x90\x00"));
    assert!(magic_matches(
        "bild.png",
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00]
    ));
    assert!(!magic_matches("bild.png", b"GIF89a"));
    assert!(magic_matches("foto.JPG", &[0xFF, 0xD8, 0xFF, 0xE0]));
    assert!(!magic_matches("foto.jpeg", b"%PDF"));
    assert!(magic_matches("anim.webp", b"RIFF\x00\x00\x00\x00WEBPVP8 "));
    assert!(!magic_matches("anim.webp", b"RIFF\x00\x00\x00\x00WAVEdata"));
    // Non-previewable types stay extension-only.
    assert!(magic_matches("daten.zip", b"whatever"));
    assert!(magic_matches("noextension", b""));
}

#[test]
fn cidr_matching_works() {
    let v4 = IpCidr::parse("10.0.0.0/8").unwrap();
    assert!(v4.contains("10.250.1.2".parse().unwrap()));
    assert!(!v4.contains("11.0.0.1".parse().unwrap()));
    assert!(!v4.contains("::1".parse().unwrap()));
    let single = IpCidr::parse("192.168.1.5").unwrap();
    assert!(single.contains("192.168.1.5".parse().unwrap()));
    assert!(!single.contains("192.168.1.6".parse().unwrap()));
    let v6 = IpCidr::parse("fc00::/7").unwrap();
    assert!(v6.contains("fd12:3456::1".parse().unwrap()));
    assert!(!v6.contains("2001:db8::1".parse().unwrap()));
    assert!(IpCidr::parse("10.0.0.0/33").is_none());
    assert!(IpCidr::parse("nonsense").is_none());
}

#[test]
fn file_names_are_sanitized() {
    assert_eq!(sanitize_file_name("../bad name.pdf"), "bad_name.pdf");
}

#[test]
fn upload_extensions_are_allowlisted() {
    assert!(allowed_upload_extension("plan.pdf"));
    assert!(allowed_upload_extension("PHOTO.JPG"));
    assert!(allowed_upload_extension("modell.ifc"));
    assert!(!allowed_upload_extension("malware.exe"));
    assert!(!allowed_upload_extension("seite.html"));
    assert!(!allowed_upload_extension("noextension"));
}

#[test]
fn inline_preview_is_limited_to_safe_types() {
    assert!(inline_previewable("plan.pdf"));
    assert!(inline_previewable("PHOTO.JPG"));
    assert!(inline_previewable("foto.webp"));
    // SVG can execute script when rendered as a document.
    assert!(!inline_previewable("logo.svg"));
    assert!(!inline_previewable("daten.xlsx"));
}

#[test]
fn shift_date_advances_by_recurrence() {
    let date = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    assert_eq!(
        shift_date(date, Recurrence::Daily),
        NaiveDate::from_ymd_opt(2026, 6, 2).unwrap()
    );
    assert_eq!(
        shift_date(date, Recurrence::Weekly),
        NaiveDate::from_ymd_opt(2026, 6, 8).unwrap()
    );
    assert_eq!(
        shift_date(date, Recurrence::Biweekly),
        NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()
    );
    assert_eq!(
        shift_date(date, Recurrence::Monthly),
        NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
    );
}

#[test]
fn shift_date_monthly_clamps_to_month_end() {
    let date = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
    assert_eq!(
        shift_date(date, Recurrence::Monthly),
        NaiveDate::from_ymd_opt(2026, 2, 28).unwrap()
    );
}

#[test]
fn mentions_match_exact_names_with_boundaries() {
    let anna = Uuid::new_v4();
    let anna_schmidt = Uuid::new_v4();
    let joerg = Uuid::new_v4();
    let members = vec![
        (anna, "Anna".to_string()),
        (anna_schmidt, "Anna Schmidt".to_string()),
        (joerg, "Jörg Müller".to_string()),
    ];

    // Longest name wins; the shorter prefix member is not also mentioned.
    assert_eq!(
        mentioned_user_ids("ping @Anna Schmidt bitte prüfen", &members),
        vec![anna_schmidt]
    );
    assert_eq!(mentioned_user_ids("@Anna kannst du?", &members), vec![anna]);
    // Boundary check: a longer word must not match a shorter name.
    assert!(mentioned_user_ids("@Annabelle hi", &members).is_empty());
    // Umlaut names work without any lowercasing tricks.
    assert_eq!(
        mentioned_user_ids("cc @Jörg Müller!", &members),
        vec![joerg]
    );
    // No mention syntax, no hits.
    assert!(mentioned_user_ids("mail an anna@example.com", &members).is_empty());
    // Duplicates collapse.
    assert_eq!(
        mentioned_user_ids("@Anna und nochmal @Anna", &members),
        vec![anna]
    );
}

#[test]
fn viewer_cannot_edit_but_members_and_up_can() {
    assert!(Role::Owner.can_edit());
    assert!(Role::Admin.can_edit());
    assert!(Role::Member.can_edit());
    assert!(!Role::Viewer.can_edit());

    assert!(Role::Owner.can_admin());
    assert!(Role::Admin.can_admin());
    assert!(!Role::Member.can_admin());
    assert!(!Role::Viewer.can_admin());
}

#[test]
fn monthly_recurrence_preserves_task_duration() {
    let jan_1 = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let jan_31 = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
    let feb_1 = NaiveDate::from_ymd_opt(2026, 2, 1).unwrap();
    let mar_3 = NaiveDate::from_ymd_opt(2026, 3, 3).unwrap();
    assert_eq!(
        shifted_due_date(Some(jan_1), Some(jan_31), Recurrence::Monthly),
        Some(mar_3)
    );
    assert_eq!(
        shifted_start_date(Some(jan_1), Recurrence::Monthly),
        Some(feb_1)
    );

    // Same-day monthly task clamps both dates to month-end.
    assert_eq!(
        shifted_due_date(Some(jan_31), Some(jan_31), Recurrence::Monthly),
        Some(NaiveDate::from_ymd_opt(2026, 2, 28).unwrap())
    );
}

#[test]
fn fixed_recurrences_shift_start_and_due_by_same_step() {
    let jan_1 = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let jan_5 = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
    assert_eq!(
        shifted_due_date(Some(jan_1), Some(jan_5), Recurrence::Daily),
        Some(NaiveDate::from_ymd_opt(2026, 1, 6).unwrap())
    );
    assert_eq!(
        shifted_due_date(Some(jan_1), Some(jan_5), Recurrence::Weekly),
        Some(NaiveDate::from_ymd_opt(2026, 1, 12).unwrap())
    );
    assert_eq!(
        shifted_due_date(Some(jan_1), Some(jan_5), Recurrence::Biweekly),
        Some(NaiveDate::from_ymd_opt(2026, 1, 19).unwrap())
    );
}

fn test_config() -> AppConfig {
    AppConfig {
        bind: "127.0.0.1:0".into(),
        static_dir: PathBuf::from("."),
        upload_dir: PathBuf::from("."),
        session_secret: "test-secret-with-at-least-32-characters!".into(),
        cookie_secure: false,
        seed_demo: false,
        registration_enabled: true,
        max_workspace_storage_bytes: MAX_WORKSPACE_STORAGE_BYTES,
        trust_proxy: false,
        trusted_proxies: default_trusted_proxies(),
        public_origin: None,
    }
}

fn headers_with_cookie(cookie: &str) -> HeaderMap {
    let pair = cookie.split(';').next().expect("cookie pair");
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, HeaderValue::from_str(pair).expect("valid header"));
    headers
}

fn origin_headers(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(origin) = origin {
        headers.insert(ORIGIN, HeaderValue::from_str(origin).expect("valid header"));
    }
    if let Some(host) = host {
        headers.insert(HOST, HeaderValue::from_str(host).expect("valid header"));
    }
    headers
}

#[test]
fn same_origin_compares_against_host_header() {
    let cfg = test_config();
    // No Origin header (curl, server-to-server): allowed.
    assert!(same_origin(
        &cfg,
        &origin_headers(None, Some("example.com"))
    ));
    assert!(same_origin(
        &cfg,
        &origin_headers(Some("https://example.com"), Some("example.com"))
    ));
    assert!(!same_origin(
        &cfg,
        &origin_headers(Some("https://example.com:8443"), Some("example.com:443"))
    ));
    assert!(same_origin(
        &cfg,
        &origin_headers(Some("https://example.com:8443"), Some("example.com:8443"))
    ));
    assert!(same_origin(
        &cfg,
        &origin_headers(Some("https://[::1]:8080"), Some("[::1]:8080"))
    ));
    assert!(!same_origin(
        &cfg,
        &origin_headers(Some("https://evil.test"), Some("example.com"))
    ));
    assert!(!same_origin(
        &cfg,
        &origin_headers(Some("null"), Some("example.com"))
    ));
    assert!(!same_origin(
        &cfg,
        &origin_headers(Some("https://example.com"), None)
    ));
}

#[test]
fn same_origin_requires_exact_public_origin() {
    let mut cfg = test_config();
    cfg.public_origin = Some("https://kowobau.example".into());
    assert!(same_origin(
        &cfg,
        &origin_headers(Some("https://kowobau.example"), Some("other-host"))
    ));
    assert!(same_origin(
        &cfg,
        &origin_headers(Some("HTTPS://KOWOBAU.EXAMPLE"), None)
    ));
    assert!(!same_origin(
        &cfg,
        &origin_headers(Some("http://kowobau.example"), Some("kowobau.example"))
    ));
    assert!(!same_origin(
        &cfg,
        &origin_headers(Some("https://evil.test"), Some("kowobau.example"))
    ));
}

#[test]
fn session_cookie_roundtrip() {
    let cfg = test_config();
    let session_id = Uuid::new_v4();
    let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
    let headers = headers_with_cookie(&cookie);
    let parsed = parse_session_cookie(&headers, &cfg).expect("cookie parses");
    assert_eq!(parsed, session_id);
}

#[test]
fn tampered_signature_is_rejected() {
    let cfg = test_config();
    let session_id = Uuid::new_v4();
    let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
    let other_id = Uuid::new_v4().to_string();
    let signature = cookie
        .split(';')
        .next()
        .and_then(|pair| pair.rsplit_once('.'))
        .map(|(_, sig)| sig.to_string())
        .expect("signature present");
    let forged = format!("{COOKIE_NAME}={other_id}.{signature}");
    let headers = headers_with_cookie(&forged);
    assert!(matches!(
        parse_session_cookie(&headers, &cfg),
        Err(AppError::Unauthorized)
    ));
}

#[test]
fn secure_cookie_uses_host_prefix_and_roundtrips() {
    let mut cfg = test_config();
    cfg.cookie_secure = true;
    let session_id = Uuid::new_v4();
    let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
    assert!(cookie.starts_with("__Host-kowobau_session="));
    assert!(cookie.contains("; Secure"));
    assert!(cookie.contains("Path=/"));
    assert!(!cookie.contains("Domain="));
    let headers = headers_with_cookie(&cookie);
    assert_eq!(
        parse_session_cookie(&headers, &cfg).expect("cookie parses"),
        session_id
    );
    assert!(expired_cookie(&cfg).starts_with("__Host-kowobau_session=;"));
}

#[test]
fn invite_tokens_hash_deterministically_and_differ() {
    let a = generate_invite_token();
    let b = generate_invite_token();
    assert_ne!(a, b);
    assert_eq!(invite_token_hash(&a), invite_token_hash(&a));
    assert_ne!(invite_token_hash(&a), invite_token_hash(&b));
}

#[test]
fn cookie_with_wrong_secret_is_rejected() {
    let cfg = test_config();
    let session_id = Uuid::new_v4();
    let cookie = build_cookie(&cfg, session_id).expect("cookie builds");
    let mut other_cfg = test_config();
    other_cfg.session_secret = "another-secret-with-at-least-32-chars!!".into();
    let headers = headers_with_cookie(&cookie);
    assert!(matches!(
        parse_session_cookie(&headers, &other_cfg),
        Err(AppError::Unauthorized)
    ));
}
