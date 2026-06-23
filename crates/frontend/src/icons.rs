use crate::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppIcon {
    Alert,
    Bell,
    Calendar,
    CheckCircle,
    Clock,
    Dashboard,
    Flag,
    Kanban,
    Roadmap,
    Search,
    Settings,
    Sliders,
    Ticket,
    Timeline,
    Users,
}

pub(crate) fn app_icon(icon: AppIcon) -> View {
    match icon {
        AppIcon::Alert => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M12 8v5"></path>
                <path d="M12 17h.01"></path>
                <path d="M10.3 4.9 3.1 17.3A2 2 0 0 0 4.8 20h14.4a2 2 0 0 0 1.7-2.7L13.7 4.9a2 2 0 0 0-3.4 0Z"></path>
            </svg>
        }.into_view(),
        AppIcon::Bell => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <g transform="translate(1.8 3.3) scale(0.85)" stroke-width="2.24">
                    <path d="M18 10.5A6 6 0 0 0 6 10.5c0 7-3 9-3 9h18s-3-2-3-9"></path>
                    <path d="M13.7 22a2 2 0 0 1-3.4 0"></path>
                </g>
            </svg>
        }.into_view(),
        AppIcon::Calendar => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M8 3v4"></path>
                <path d="M16 3v4"></path>
                <path d="M4 9h16"></path>
                <rect x="4" y="5" width="16" height="16" rx="3"></rect>
                <path d="M8 13h.01"></path>
                <path d="M12 13h.01"></path>
                <path d="M16 13h.01"></path>
            </svg>
        }.into_view(),
        AppIcon::CheckCircle => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="8"></circle>
                <path d="m8.7 12.2 2.1 2.1 4.6-4.9"></path>
            </svg>
        }.into_view(),
        AppIcon::Clock => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="8"></circle>
                <path d="M12 8v4l3 2"></path>
            </svg>
        }.into_view(),
        AppIcon::Dashboard => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <rect x="4" y="4" width="7" height="7" rx="2"></rect>
                <rect x="13" y="4" width="7" height="5" rx="2"></rect>
                <rect x="13" y="11" width="7" height="9" rx="2"></rect>
                <rect x="4" y="13" width="7" height="7" rx="2"></rect>
            </svg>
        }.into_view(),
        AppIcon::Flag => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M6 21V4"></path>
                <path d="M6 5h10l-1.4 4L16 13H6"></path>
            </svg>
        }.into_view(),
        AppIcon::Kanban => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <rect x="4" y="4" width="5" height="16" rx="2"></rect>
                <rect x="10.5" y="4" width="5" height="10" rx="2"></rect>
                <rect x="17" y="4" width="3" height="13" rx="1.5"></rect>
            </svg>
        }.into_view(),
        AppIcon::Roadmap => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M5 19c2.8 0 2.8-4 5.6-4h2.8c2.8 0 2.8-4 5.6-4"></path>
                <circle cx="5" cy="19" r="2"></circle>
                <circle cx="12" cy="15" r="2"></circle>
                <path d="M17 4v8"></path>
                <path d="M17 5h4l-1 2 1 2h-4"></path>
            </svg>
        }.into_view(),
        AppIcon::Search => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="11" cy="11" r="6"></circle>
                <path d="m16 16 4 4"></path>
            </svg>
        }.into_view(),
        AppIcon::Settings => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="12" cy="12" r="3"></circle>
                <path d="M12 3v3"></path>
                <path d="M12 18v3"></path>
                <path d="M3 12h3"></path>
                <path d="M18 12h3"></path>
                <path d="m5.6 5.6 2.1 2.1"></path>
                <path d="m16.3 16.3 2.1 2.1"></path>
                <path d="m18.4 5.6-2.1 2.1"></path>
                <path d="m7.7 16.3-2.1 2.1"></path>
            </svg>
        }.into_view(),
        AppIcon::Sliders => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M4 6h16"></path>
                <path d="M4 12h16"></path>
                <path d="M4 18h16"></path>
                <circle cx="9" cy="6" r="2"></circle>
                <circle cx="15" cy="12" r="2"></circle>
                <circle cx="8" cy="18" r="2"></circle>
            </svg>
        }.into_view(),
        AppIcon::Ticket => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M4 8a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v2.2a2 2 0 0 0 0 3.6V16a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-2.2a2 2 0 0 0 0-3.6V8Z"></path>
                <path d="M9 8v8"></path>
            </svg>
        }.into_view(),
        AppIcon::Timeline => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <path d="M5 7h7"></path>
                <path d="M5 12h14"></path>
                <path d="M5 17h10"></path>
                <path d="M4 5v14"></path>
            </svg>
        }.into_view(),
        AppIcon::Users => view! {
            <svg class="app-icon" aria-hidden="true" focusable="false" viewBox="0 0 24 24">
                <circle cx="9" cy="8" r="3"></circle>
                <path d="M4 19a5 5 0 0 1 10 0"></path>
                <path d="M16 11a2.5 2.5 0 0 0 0-5"></path>
                <path d="M17 19a4 4 0 0 0-3-3.8"></path>
            </svg>
        }.into_view(),
    }
}
