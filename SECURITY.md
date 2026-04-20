# Security Policy

## Reporting a Vulnerability

Do not report security vulnerabilities through public GitHub issues, pull requests, or discussions.

### Preferred: GitHub Security Advisories

Use **GitHub Security Advisories** to report vulnerabilities privately:

1. Go to the [Writ repository](https://github.com/ibrahemid/writ) on GitHub.
2. Click the **Security** tab.
3. Click **Report a vulnerability**.
4. Fill in the advisory form with as much detail as possible.

### Alternative: Private Email

If GitHub Security Advisories are not available to you, email the maintainer privately at:

**ibrahemid@gmail.com**

Use the subject line `[writ security]` and include:

- A description of the issue and its potential impact.
- Steps to reproduce, or a proof-of-concept if you have one.
- The affected version (output of `writ --version` or the release tag).
- Your operating system and architecture.

## Response Expectations

- You will receive an acknowledgement within **48 hours** of your report.
- If the issue is confirmed, a patch will be prepared and released within **14 days** for critical issues or **30 days** for lower-severity issues.
- You will be kept informed throughout the process and credited in the advisory unless you request otherwise.
- Please allow us a reasonable window to ship a fix before public disclosure.

## Supported Versions

Only the most recent release on the `main` branch receives security fixes. We do not backport patches to older releases.

| Version | Supported          |
| ------- | ------------------ |
| latest `0.x` release | Yes   |
| older `0.x` releases | No    |

Once Writ reaches a `1.x` release, this table will be updated with a formal support window.

## Scope

The following are in scope:

- `writ-core`, `writ-storage`, and `writ-plugin` Rust crates.
- The `src-tauri` Tauri IPC layer and its command surface.
- The SolidJS frontend under `src/`.
- SQLite database handling and FTS5 query construction.
- Release artifacts published under GitHub Releases.

The following are out of scope:

- Vulnerabilities in third-party dependencies (please report those upstream first).
- Issues that require physical access to the user's machine.
- Social engineering attacks.
- Denial of service via resource exhaustion on the user's own machine.
