use crate::*;

/// CSRF defense-in-depth on top of SameSite=Lax: browser-sent state-changing
/// requests must come from our own origin. Requests without an Origin header
/// (curl, server-to-server) are allowed through. With `MILESTEP_PUBLIC_ORIGIN`
/// set, the full origin (including scheme) must match exactly; otherwise we
/// fall back to comparing the Origin host against the Host header.
pub(crate) async fn enforce_same_origin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && !same_origin(&state.cfg, req.headers())
    {
        return Err(AppError::Forbidden);
    }
    Ok(next.run(req).await)
}

/// True when the request's Origin header (if present) matches our own origin.
/// Requests without an Origin header (curl, server-to-server) pass. With
/// `MILESTEP_PUBLIC_ORIGIN` set, the full origin (including scheme) must match
/// exactly; otherwise the Origin host is compared against the Host header.
pub(crate) fn same_origin(cfg: &AppConfig, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    if let Some(expected) = &cfg.public_origin {
        return origin.eq_ignore_ascii_case(expected);
    }
    // Compare the full authority (host:port) so origins on different ports are
    // not treated as same-origin. IPv6 addresses keep their brackets, matching
    // the Host header format.
    let origin_authority = origin.split_once("://").map_or(origin, |(_, a)| a);
    let request_host = headers
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    origin != "null"
        && !request_host.is_empty()
        && origin_authority.eq_ignore_ascii_case(request_host)
}

/// Fixed-window per-IP limiter for the unauthenticated auth endpoints. These
/// trigger expensive Argon2 hashing and are the brute-force surface, so they
/// get a much tighter budget than the rest of the API.
// The lock is already confined to the smallest possible block; the lint's
// suggested restructuring would not shrink the critical section further.
#[allow(clippy::significant_drop_tightening)]
pub(crate) async fn rate_limit_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let ip = client_ip(&state, &req).ok_or(AppError::TooManyRequests)?;
    {
        let mut limiter = state.auth_limiter.lock().expect("limiter lock poisoned");
        let now = Instant::now();
        // Opportunistic pruning keeps the map from growing with one entry per
        // IP ever seen.
        limiter.retain(|_, (start, _)| now.duration_since(*start) < AUTH_RATE_LIMIT_WINDOW);
        let entry = limiter.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= AUTH_RATE_LIMIT_WINDOW {
            *entry = (now, 0);
        }
        entry.1 += 1;
        if entry.1 > AUTH_RATE_LIMIT_MAX_ATTEMPTS {
            return Err(AppError::TooManyRequests);
        }
    }
    Ok(next.run(req).await)
}

pub(crate) fn client_ip(state: &AppState, req: &Request) -> Option<IpAddr> {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    if state.cfg.trust_proxy {
        // X-Real-IP is only believable when the request actually came through
        // one of our proxies; anyone else gets rate limited by peer address.
        let peer_is_trusted =
            peer.is_some_and(|ip| state.cfg.trusted_proxies.iter().any(|c| c.contains(ip)));
        if peer_is_trusted {
            if let Some(ip) = req
                .headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok())
            {
                return Some(ip);
            }
        }
    }
    peer
}

/// Minimal CIDR matcher for the trusted-proxy list; hand-rolled to avoid a
/// dependency for ~30 lines of bit twiddling.
#[derive(Debug, Clone)]
pub(crate) struct IpCidr {
    pub(crate) addr: IpAddr,
    pub(crate) prefix: u8,
}

impl IpCidr {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let (addr, prefix) = if let Some((addr, prefix)) = value.split_once('/') {
            (addr.parse().ok()?, prefix.parse().ok()?)
        } else {
            let addr: IpAddr = value.parse().ok()?;
            let full = if addr.is_ipv4() { 32 } else { 128 };
            (addr, full)
        };
        let max = if addr.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { addr, prefix })
    }

    pub(crate) fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from_be_bytes(net.octets()) & mask == u32::from_be_bytes(ip.octets()) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from_be_bytes(net.octets()) & mask == u128::from_be_bytes(ip.octets()) & mask
            }
            _ => false,
        }
    }
}

/// Loopback plus the RFC 1918 / ULA private ranges — where a reverse proxy
/// in a Docker network or on the same host actually lives.
pub(crate) fn default_trusted_proxies() -> Vec<IpCidr> {
    [
        "127.0.0.0/8",
        "::1/128",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "fc00::/7",
    ]
    .iter()
    .map(|c| IpCidr::parse(c).expect("static CIDR is valid"))
    .collect()
}

// No 'unsafe-inline' for scripts: trunk's inline bootstrap is externalized to
// /init.js by crates/frontend/externalize-init.py after every build.
pub(crate) const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
     connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Defense-in-depth security headers so a directly exposed backend (without
/// the nginx in front) still serves a hardened SPA.
pub(crate) async fn security_headers(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // Handlers may set stricter, response-specific framing/CSP headers (inline
    // attachment previews need SAMEORIGIN framing); only fill in the defaults.
    if !headers.contains_key(X_FRAME_OPTIONS) {
        headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    if !headers.contains_key(CONTENT_SECURITY_POLICY) {
        headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    }
    headers.insert(
        REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    res
}
