use crate::*;

pub(crate) fn upload_attachments(
    task_id: String,
    files: web_sys::FileList,
    set_uploading: WriteSignal<bool>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) {
    let Ok(form) = web_sys::FormData::new() else {
        return;
    };
    let mut any = false;
    for i in 0..files.length() {
        if let Some(file) = files.get(i) {
            if form.append_with_blob("files", &file).is_ok() {
                any = true;
            }
        }
    }
    if !any {
        return;
    }
    set_uploading.set(true);
    spawn_local(async move {
        match api_post_form::<TaskDto>(&format!("/api/tasks/{task_id}/attachments"), form).await {
            Ok(task) => {
                replace_task(set_data, task);
                set_error.set(None);
            }
            Err(err) => set_error.set(Some(err.message)),
        }
        set_uploading.set(false);
    });
}

pub(crate) fn attachment_ext(file_name: &str) -> String {
    file_extension_lowercase(file_name).unwrap_or_default()
}

/// Attachment chip plus an inline preview: images render directly, PDFs get
/// a toggleable iframe, everything else stays a plain download link.
pub(crate) fn attachment_view(
    a: AttachmentDto,
    lang: ReadSignal<Lang>,
    // Editing signal + delete callback; None hides the delete control entirely.
    delete: Option<(ReadSignal<bool>, Callback<String>)>,
) -> View {
    let ext = attachment_ext(&a.file_name);
    let inline_url = format!("/api/attachments/{}?inline=1", a.id);
    let download_url = format!("/api/attachments/{}", a.id);
    let delete_btn = delete.map(|(editing, on_delete)| {
        let attachment_id = a.id.clone();
        let file_name = a.file_name.clone();
        view! {
            {move || editing.get().then(|| {
                let attachment_id = attachment_id.clone();
                let file_name = file_name.clone();
                view! {
                    <button class="danger-link" on:click=move |_| {
                        if confirm_delete_attachment(&file_name, lang.get_untracked()) {
                            on_delete.call(attachment_id.clone());
                        }
                    }>{move || lang.get().tr("Loeschen", "Delete")}</button>
                }
            })}
        }
    });
    let chip = view! {
        <span class="file-chip-row">
            <a class="file-chip" href=download_url download>"Datei "{a.file_name.clone()}<small>{a.size_label.clone()}</small></a>
            {delete_btn}
        </span>
    };
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => {
            let alt = a.file_name;
            view! {
                <div class="attachment">
                    {chip}
                    <a href=inline_url.clone() target="_blank" rel="noopener">
                        <img class="attach-preview" src=inline_url loading="lazy" alt=alt/>
                    </a>
                </div>
            }
            .into_view()
        }
        "pdf" => {
            let (preview, set_preview) = create_signal(false);
            let title = a.file_name;
            view! {
                <div class="attachment">
                    {chip}
                    <button class="link-button" on:click=move |_| set_preview.update(|p| *p = !*p)>
                        {move || match (preview.get(), lang.get().is_de()) {
                            (true, true) => "Vorschau ausblenden",
                            (true, false) => "Hide preview",
                            (false, true) => "Vorschau anzeigen",
                            (false, false) => "Show preview",
                        }}
                    </button>
                    {move || preview.get().then(|| view! {
                        <iframe class="attach-pdf" src=inline_url.clone() title=title.clone()></iframe>
                    })}
                </div>
            }
            .into_view()
        }
        _ => view! { <div class="attachment">{chip}</div> }.into_view(),
    }
}

/// The trailing "@query" fragment of the comment draft, if the cursor sits in
/// one: returns the byte offset of the '@' and the query after it. The '@'
/// must start the input or follow whitespace.
pub(crate) fn mention_query(value: &str) -> Option<(usize, String)> {
    let at = value.rfind('@')?;
    if value[..at]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace())
    {
        return None;
    }
    let query = &value[at + 1..];
    if query.len() > 30 {
        return None;
    }
    Some((at, query.to_string()))
}

/// Splits a comment body into (text, `is_mention`) segments. Uses the same
/// rule as the backend parser: exact member names, longest first, followed by
/// a non-alphanumeric boundary.
pub(crate) fn mention_segments(body: &str, names: &[String]) -> Vec<(String, bool)> {
    let mut by_length: Vec<&String> = names.iter().filter(|n| !n.trim().is_empty()).collect();
    by_length.sort_by_key(|name| std::cmp::Reverse(name.len()));

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for name in by_length {
        let pattern = format!("@{name}");
        for (start, _) in body.match_indices(&pattern) {
            let end = start + pattern.len();
            let boundary_ok = body[end..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            let overlaps = ranges.iter().any(|&(s, e)| start < e && end > s);
            if boundary_ok && !overlaps {
                ranges.push((start, end));
            }
        }
    }
    ranges.sort_unstable();

    let mut out = Vec::new();
    let mut cursor = 0;
    for (start, end) in ranges {
        if start > cursor {
            out.push((body[cursor..start].to_string(), false));
        }
        out.push((body[start..end].to_string(), true));
        cursor = end;
    }
    if cursor < body.len() {
        out.push((body[cursor..].to_string(), false));
    }
    out
}

/// Renders a comment body with member mentions highlighted. Builds views from
/// plain segments (never raw HTML), so bodies stay XSS-safe.
pub(crate) fn comment_body_view(body: &str, member_names: &[String]) -> View {
    mention_segments(body, member_names)
        .into_iter()
        .map(|(text, is_mention)| {
            if is_mention {
                view! { <span class="mention">{text}</span> }.into_view()
            } else {
                text.into_view()
            }
        })
        .collect_view()
}
