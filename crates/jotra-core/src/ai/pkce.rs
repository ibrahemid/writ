//! The pure half of the OpenRouter PKCE connect flow (ADR-040 section 6).
//!
//! Everything here is a function over bytes and strings: the verifier is built
//! from random bytes the caller supplies, the challenge is a hash, the
//! authorization URL is a format, and the callback is parsed out of one line of
//! an HTTP request. The socket, the browser and the key exchange live in
//! `src-tauri`, so this module is tested against the RFC's own vector without a
//! network and without a runtime.
//!
//! The verifier never reaches a log line or an error string: [`Verifier`]'s
//! `Debug` prints no digits, and no error variant here carries a value read off
//! the wire.

use std::fmt;

use crate::hash::sha256_bytes;

/// Where the user's browser is sent to authorize.
const AUTH_URL: &str = "https://openrouter.ai/auth";

/// The unreserved base64url alphabet, RFC 4648 section 5. Padding is never
/// written: RFC 7636 requires the verifier and the challenge to be padless.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding.
///
/// Written out rather than taken from a crate: it is a dozen lines, and the two
/// values that pass through it (the verifier and the challenge) are the two the
/// flow cannot get wrong.
fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b1 = u32::from(chunk[0]);
        let b2 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b3 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let packed = (b1 << 16) | (b2 << 8) | b3;
        // Three bytes make four characters; two make three, one makes two.
        let written = chunk.len() + 1;
        for shift in [18u32, 12, 6, 0].into_iter().take(written) {
            let index = ((packed >> shift) & 63) as usize;
            out.push(char::from(ALPHABET[index]));
        }
    }
    out
}

/// The PKCE code verifier for one connect flow.
///
/// Owns its string so the only way to read it is [`Verifier::as_str`], and
/// prints nothing in `Debug`: a verifier in a log line is the credential half
/// of the exchange.
#[derive(Clone, PartialEq, Eq)]
pub struct Verifier(String);

impl Verifier {
    /// Builds a verifier from 32 random bytes, base64url encoded without
    /// padding. That is 43 characters of the unreserved alphabet, inside RFC
    /// 7636's 43-to-128 range.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(base64url(bytes))
    }

    /// The verifier as it goes on the wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Verifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Verifier(..)")
    }
}

/// The S256 challenge for a verifier: base64url of its SHA-256 digest, no
/// padding.
pub fn challenge_s256(verifier: &str) -> String {
    base64url(&sha256_bytes(verifier.as_bytes()))
}

/// An opaque value for one flow, from random bytes. The callback must carry it
/// back unchanged, which is what ties the request the listener accepts to the
/// flow the user started. Base64url, so it sits in a URL path as it is.
pub fn state_from_bytes(bytes: &[u8; 16]) -> String {
    base64url(bytes)
}

/// Where the browser is sent back to: the loopback port, with this flow's
/// state as the last segment of the path.
///
/// OpenRouter's flow takes `callback_url`, `code_challenge` and
/// `code_challenge_method` and appends `code` to the callback on the way back.
/// A `state` parameter of its own comes back only when the user authorizes. A
/// state carried in the callback URL itself comes back with the callback
/// whatever the provider echoes, so the check never rests on an optional
/// parameter.
pub fn callback_url(port: u16, state: &str) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PREFIX}{state}")
}

/// The path every callback starts with; the state follows it.
const CALLBACK_PREFIX: &str = "/callback/";

/// The URL the user's browser is sent to.
///
/// Every value is form-encoded through the `url` crate, so a callback URL
/// carrying a port and a path survives the round trip intact.
pub fn auth_url(callback_url: &str, challenge: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("callback_url", callback_url)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .finish();
    format!("{AUTH_URL}?{query}")
}

/// What the browser handed back on the loopback socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    /// The authorization code, to be exchanged for a key.
    pub code: String,
    /// The state the flow sent, as it came back in the path.
    pub state: String,
}

/// Why a request line is not the callback this flow is waiting for.
///
/// Every variant means the same thing to the user, and none of them carries a
/// value read off the socket. They are separate so the tests can say which
/// rule rejected a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackError {
    /// The line is not a request line at all.
    Malformed,
    /// Something other than `GET`.
    NotGet,
    /// A path other than `/callback/<state>`.
    WrongPath,
    /// No `code` parameter.
    MissingCode,
    /// `/callback` with no state after it.
    MissingState,
}

impl fmt::Display for CallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self {
            Self::Malformed => "malformed request line",
            Self::NotGet => "not a GET",
            Self::WrongPath => "wrong path",
            Self::MissingCode => "no code",
            Self::MissingState => "no state",
        };
        f.write_str(reason)
    }
}

impl std::error::Error for CallbackError {}

/// Reads the first line of an HTTP request: `GET /callback/<state>?code=… HTTP/1.1`.
///
/// Only a `GET` of `/callback/<state>` carrying a `code` is a callback. The
/// method and the path are checked before the query, so a stray request from
/// another program on the machine is rejected on its shape rather than on its
/// contents. A `state` query parameter is not read: the path is the one place
/// the state travels.
pub fn parse_callback(request_line: &str) -> Result<Callback, CallbackError> {
    let (state, query) = callback_target(request_line)?;
    let code = url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, value)| key == "code" && !value.is_empty())
        .map(|(_, value)| value.into_owned())
        .ok_or(CallbackError::MissingCode)?;

    Ok(Callback {
        code,
        state: state.to_string(),
    })
}

/// The state a callback carries in its path, whether or not it carries a code.
///
/// A browser sent back without a code, which is what a denial is, still names
/// the flow it belongs to here, so the receiver can end that flow rather than
/// wait out its deadline.
pub fn callback_state(request_line: &str) -> Option<&str> {
    callback_target(request_line).ok().map(|(state, _)| state)
}

/// The state in the path and the query after it, for a `GET` of
/// `/callback/<state>`.
fn callback_target(request_line: &str) -> Result<(&str, &str), CallbackError> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(CallbackError::Malformed)?;
    let target = parts.next().ok_or(CallbackError::Malformed)?;
    if method != "GET" {
        return Err(CallbackError::NotGet);
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let state = match path.strip_prefix(CALLBACK_PREFIX) {
        Some("") => return Err(CallbackError::MissingState),
        Some(state) if !state.contains('/') => state,
        Some(_) => return Err(CallbackError::WrongPath),
        None if path == "/callback" => return Err(CallbackError::MissingState),
        None => return Err(CallbackError::WrongPath),
    };
    Ok((state, query))
}

/// Whether the callback carried this flow's state, compared in constant time.
///
/// Lengths first, then a fold over every byte: the loop never stops early, so
/// how long the comparison takes says nothing about how much of the state a
/// caller guessed right.
pub fn check_state(expected: &str, got: &str) -> bool {
    let expected = expected.as_bytes();
    let got = got.as_bytes();
    if expected.len() != got.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in expected.iter().zip(got.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 appendix B: the verifier and the challenge the specification
    /// itself publishes. If the encoder or the hash drifts, this is what says
    /// so before a real provider does.
    #[test]
    fn the_rfc_vector_derives_its_published_challenge() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_s256(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_verifier_is_forty_three_unreserved_characters() {
        let verifier = Verifier::from_bytes(&[0xA7; 32]);
        assert_eq!(verifier.as_str().len(), 43);
        assert!(verifier.as_str().bytes().all(|b| ALPHABET.contains(&b)));
        assert!(!verifier.as_str().contains('='));
    }

    #[test]
    fn different_bytes_make_different_verifiers() {
        let a = Verifier::from_bytes(&[0u8; 32]);
        let mut raw = [0u8; 32];
        raw[31] = 1;
        let b = Verifier::from_bytes(&raw);
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn a_verifier_prints_nothing_of_itself() {
        let verifier = Verifier::from_bytes(&[0x5A; 32]);
        let printed = format!("{verifier:?}");
        assert_eq!(printed, "Verifier(..)");
        assert!(!printed.contains(verifier.as_str()));
    }

    #[test]
    fn base64url_matches_the_known_encodings() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(b"foob"), "Zm9vYg");
        assert_eq!(base64url(&[0xFB, 0xFF, 0xFE]), "-__-");
    }

    #[test]
    fn a_state_is_twenty_two_characters() {
        assert_eq!(state_from_bytes(&[0x11; 16]).len(), 22);
    }

    #[test]
    fn the_auth_url_carries_the_three_parameters_encoded() {
        let url = auth_url(
            &callback_url(52341, "abc-def"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        );
        assert!(url.starts_with("https://openrouter.ai/auth?"));
        assert!(url.contains("callback_url=http%3A%2F%2F127.0.0.1%3A52341%2Fcallback%2Fabc-def"));
        assert!(url.contains("code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains("&state="), "{url}");
    }

    #[test]
    fn the_callback_url_carries_the_state_in_its_path() {
        let state = state_from_bytes(&[0xFB; 16]);
        let url = callback_url(52341, &state);
        assert_eq!(url, format!("http://127.0.0.1:52341/callback/{state}"));
        // What the browser asks for when it comes back is that path.
        let line = format!("GET /callback/{state}?code=abc HTTP/1.1");
        assert_eq!(parse_callback(&line).unwrap().state, state);
    }

    #[test]
    fn a_state_carried_in_the_path_reads_as_the_callback() {
        let parsed = parse_callback("GET /callback/st4te?code=abc123 HTTP/1.1").unwrap();
        assert_eq!(parsed.code, "abc123");
        assert_eq!(parsed.state, "st4te");
    }

    #[test]
    fn a_state_query_parameter_is_not_where_the_state_is_read() {
        let parsed = parse_callback("GET /callback/st4te?code=abc&state=other HTTP/1.1").unwrap();
        assert_eq!(parsed.state, "st4te");
        assert_eq!(
            parse_callback("GET /callback?code=abc&state=st4te HTTP/1.1"),
            Err(CallbackError::MissingState)
        );
    }

    #[test]
    fn a_percent_encoded_code_comes_back_decoded() {
        let parsed = parse_callback("GET /callback/s?code=a%2Fb HTTP/1.1").unwrap();
        assert_eq!(parsed.code, "a/b");
    }

    #[test]
    fn a_line_with_no_code_is_refused() {
        assert_eq!(
            parse_callback("GET /callback/st4te?code= HTTP/1.1"),
            Err(CallbackError::MissingCode)
        );
        assert_eq!(
            parse_callback("GET /callback/st4te HTTP/1.1"),
            Err(CallbackError::MissingCode)
        );
    }

    #[test]
    fn a_return_without_a_code_still_names_its_flow() {
        assert_eq!(
            callback_state("GET /callback/st4te?error=access_denied HTTP/1.1"),
            Some("st4te")
        );
        assert_eq!(
            callback_state("GET /callback/st4te?code=abc HTTP/1.1"),
            Some("st4te")
        );
        for line in [
            "GET /callback?error=access_denied HTTP/1.1",
            "GET /callback/?code=abc HTTP/1.1",
            "GET /callback/st4te/more HTTP/1.1",
            "POST /callback/st4te HTTP/1.1",
            "GET /favicon.ico HTTP/1.1",
            "",
        ] {
            assert_eq!(callback_state(line), None, "{line:?}");
        }
    }

    #[test]
    fn a_line_with_no_state_is_refused() {
        assert_eq!(
            parse_callback("GET /callback?code=abc HTTP/1.1"),
            Err(CallbackError::MissingState)
        );
        assert_eq!(
            parse_callback("GET /callback/?code=abc HTTP/1.1"),
            Err(CallbackError::MissingState)
        );
    }

    #[test]
    fn a_post_is_refused() {
        assert_eq!(
            parse_callback("POST /callback/st4te?code=abc HTTP/1.1"),
            Err(CallbackError::NotGet)
        );
    }

    #[test]
    fn another_path_is_refused() {
        assert_eq!(
            parse_callback("GET /?code=abc HTTP/1.1"),
            Err(CallbackError::WrongPath)
        );
        assert_eq!(
            parse_callback("GET /callbackx/st4te?code=abc HTTP/1.1"),
            Err(CallbackError::WrongPath)
        );
        assert_eq!(
            parse_callback("GET /callback/st4te/more?code=abc HTTP/1.1"),
            Err(CallbackError::WrongPath)
        );
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1"),
            Err(CallbackError::WrongPath)
        );
    }

    #[test]
    fn a_line_that_is_not_a_request_is_refused() {
        assert_eq!(parse_callback(""), Err(CallbackError::Malformed));
        assert_eq!(parse_callback("GET"), Err(CallbackError::Malformed));
    }

    #[test]
    fn a_state_matches_only_itself() {
        assert!(check_state("abc", "abc"));
        assert!(!check_state("abc", "abd"));
        assert!(!check_state("abc", "ab"));
        assert!(!check_state("abc", ""));
        assert!(check_state("", ""));
    }
}
