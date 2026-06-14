use crate::*;

/// The comments list plus the comment composer with `@`-mention autocomplete.
/// Owns its own draft/mention state and holds realtime refetches while a
/// comment is being typed, so it composes independently of the task drawer's
/// own edit-mode hold.
pub(crate) fn comments_section(
    task_id: String,
    comments: Vec<CommentDto>,
    members: Vec<MemberDto>,
    lang: ReadSignal<Lang>,
    set_data: WriteSignal<Option<BootstrapDto>>,
    set_error: WriteSignal<Option<String>>,
) -> View {
    let (comment, set_comment) = create_signal(String::new());
    let (mention_open, set_mention_open) = create_signal(false);
    let (mention_index, set_mention_index) = create_signal(0usize);
    let member_names: Vec<String> = members.iter().map(|m| m.name.clone()).collect();
    let mention_members = store_value(members);
    // A half-typed comment must not be wiped by a background refetch.
    hold_realtime_while(move || !comment.get().trim().is_empty());

    let mention_candidates = move || -> Vec<MemberDto> {
        let value = comment.get();
        let Some((_, query)) = mention_query(&value) else {
            return Vec::new();
        };
        let query = query.to_lowercase();
        mention_members.with_value(|members| {
            members
                .iter()
                .filter(|m| m.name.to_lowercase().contains(&query))
                .cloned()
                .collect()
        })
    };
    let pick_mention = move |name: String| {
        let value = comment.get_untracked();
        if let Some((at, _)) = mention_query(&value) {
            set_comment.set(format!("{}@{name} ", &value[..at]));
        }
        set_mention_open.set(false);
        set_mention_index.set(0);
    };
    let submit_comment = move || {
        let body = comment.get_untracked();
        if !body.trim().is_empty() {
            add_comment(task_id.clone(), body, set_data, set_error);
            set_comment.set(String::new());
            set_mention_open.set(false);
        }
    };
    let submit_comment_for_button = submit_comment.clone();

    view! {
        <section>
            <h3>{move || if lang.get() == Lang::De { "Kommentare" } else { "Comments" }}</h3>
            {comments.into_iter().map(|c| {
                let created = if lang.get() == Lang::De { c.created_label_de } else { c.created_label_en };
                let body = comment_body_view(&c.body, &member_names);
                view! { <div class="comment"><span class="avatar tiny">{c.author_initials}</span><p><strong>{c.author_name}</strong><br/>{body}</p><small>{created}</small></div> }
            }).collect_view()}
            <div class="comment-box">
                {move || {
                    let candidates = mention_candidates();
                    (mention_open.get() && !candidates.is_empty()).then(|| view! {
                        <div class="mention-pop">
                            {candidates.into_iter().enumerate().map(|(i, m)| {
                                let name = m.name.clone();
                                view! {
                                    <button type="button" class="mention-item" class:active=move || mention_index.get() == i
                                        on:mousedown=move |ev| {
                                            // Pick before the input loses focus.
                                            ev.prevent_default();
                                            pick_mention(name.clone());
                                        }>
                                        <span class="avatar tiny">{m.initials}</span>
                                        <span class="mention-name">{m.name}</span>
                                        <small>{m.email}</small>
                                    </button>
                                }
                            }).collect_view()}
                        </div>
                    })
                }}
                <input
                    placeholder=move || if lang.get() == Lang::De { "Kommentar schreiben... (@ erwähnt)" } else { "Write a comment... (@ mentions)" }
                    prop:value=comment
                    on:input=move |ev| {
                        let value = event_target_value(&ev);
                        set_mention_open.set(mention_query(&value).is_some());
                        set_mention_index.set(0);
                        set_comment.set(value);
                    }
                    on:keydown=move |ev| {
                        // The popup only counts as active while it has
                        // candidates; a query without matches must not
                        // swallow Enter (the user wants to submit).
                        let candidates = if mention_open.get_untracked() {
                            mention_candidates()
                        } else {
                            Vec::new()
                        };
                        if !candidates.is_empty() {
                            match ev.key().as_str() {
                                "ArrowDown" => {
                                    ev.prevent_default();
                                    set_mention_index.update(|i| *i = (*i + 1) % candidates.len());
                                }
                                "ArrowUp" => {
                                    ev.prevent_default();
                                    set_mention_index.update(|i| *i = (*i + candidates.len() - 1) % candidates.len());
                                }
                                "Enter" | "Tab" => {
                                    ev.prevent_default();
                                    let index = mention_index.get_untracked().min(candidates.len() - 1);
                                    pick_mention(candidates[index].name.clone());
                                }
                                "Escape" => set_mention_open.set(false),
                                _ => {}
                            }
                        } else if ev.key() == "Enter" {
                            submit_comment();
                        }
                    }
                />
                <button on:click=move |_| submit_comment_for_button()>"Enter"</button>
            </div>
        </section>
    }.into_view()
}
