# Security Policy

## Reporting a Vulnerability

Do not report security vulnerabilities through public GitHub issues, pull requests, or discussions.

Instead, use **GitHub Security Advisories** to report vulnerabilities privately:

1. Go to the repository on GitHub.
2. Click the **Security** tab.
3. Click **Report a vulnerability**.
4. Fill in the advisory form with as much detail as possible.

You will receive an acknowledgment within **48 hours**. If the issue is confirmed, a patch will be prepared and released within **14 days** for critical issues or **30 days** for lower-severity issues. You will be kept informed throughout the process and credited in the advisory unless you request otherwise.

## Scope

The following are in scope:

- `writ-core`, `writ-storage`, `writ-plugin` Rust crates
- `src-tauri` Tauri IPC layer
- SolidJS frontend (`src/`)
- SQLite database handling and FTS5 query construction

The following are out of scope:

- Vulnerabilities in third-party dependencies (report upstream)
- Issues requiring physical access to the machine
- Social engineering attacks

## Supported Versions

Only the latest release on `main` receives security fixes. We do not backport patches to older releases.
