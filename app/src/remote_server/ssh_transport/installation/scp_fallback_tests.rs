use super::is_loopback_url;

#[test]
fn accepts_http_loopback_urls() {
    for url in [
        "http://localhost:8080/download/cli",
        "https://127.0.0.1/download/cli",
        "http://[::1]:8080/download/cli",
    ] {
        assert!(is_loopback_url(url), "expected loopback URL: {url}");
    }
}

#[test]
fn rejects_external_and_non_http_urls() {
    for url in [
        "https://app.warp.dev/download/cli",
        "http://localhost.evil.example/download/cli",
        "http://[2001:db8::1]/download/cli",
        "file://localhost/download/cli",
        "not a URL",
    ] {
        assert!(!is_loopback_url(url), "expected rejected URL: {url}");
    }
}
