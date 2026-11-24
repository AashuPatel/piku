# Security Policy

## Supported Versions

Piku is currently in early controlled beta:

| Version | Supported |
| --- | :---: |
| `0.1.x` | Yes |

---

## Security & Privacy Scope

Piku is a local-first Windows desktop application with no cloud backend or network server. Security-relevant areas include:

- Local SQLite data handling, backup/recovery, and filesystem permissions under `%LOCALAPPDATA%\com.piku.desktop\`
- Tauri IPC command boundaries and Content Security Policy (CSP)
- Windows Registry startup handling (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Piku`)
- NSIS installer and uninstaller behavior
- Unexpected network activity or unintended collection of sensitive data (such as window titles or URLs)

---

## Reporting a Vulnerability

Please **do not** open a public GitHub Issue for a security vulnerability.

Instead, report it privately using **GitHub Security Advisories** ("Report a vulnerability" under the **Security** tab of [AashuPatel/piku](https://github.com/AashuPatel/piku/security)).

Please include:
- Affected Piku version and Windows build
- Clear steps to reproduce the issue
- Potential impact and any suggested remediation

We will acknowledge receipt and work on a fix before public disclosure.
