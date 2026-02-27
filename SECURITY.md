# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 1.x     | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in this project, please **do not** open a public GitHub issue.

Instead, report it privately using one of the following methods:

- **GitHub Private Vulnerability Reporting**: Use the [Security tab](../../security/advisories/new) on this repository to submit a private advisory.
- **Email**: Contact the maintainer directly via GitHub profile if private reporting is unavailable.

Please include as much of the following information as possible to help understand and resolve the issue quickly:

- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- Affected version(s)
- Any suggested mitigations or fixes

## Security Considerations

When deploying gluetun-connector, please note:

- The web UI should **not** be exposed directly to the public internet. Place it behind a reverse proxy with authentication (e.g. Authelia, Authentik, or HTTP Basic Auth).
- If Gluetun's HTTP control server has authentication enabled, configure `GLUETUN_API_KEY` or `GLUETUN_USER`/`GLUETUN_PASSWORD` environment variables accordingly.
- Keep the Docker image up to date to receive dependency security patches.

## Disclosure Policy

Once a fix is available, the vulnerability will be disclosed publicly in the repository's GitHub Security Advisories with credit to the reporter (unless anonymity is requested).
