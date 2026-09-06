# Security Policy

## Reporting a Vulnerability

We take the security of Local LLM Token Usage Monitor seriously. If you discover a security vulnerability, we appreciate your responsible disclosure.

### How to Report

**Preferred method:** Please use [GitHub Private Vulnerability Reporting](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/security/advisories/new) to report vulnerabilities privately.

**Alternative:** You can also reach out via [GitHub Issues](https://github.com/hajifish/Local-LLM-Token-Usage-Monitor-Desktop/issues). If the vulnerability is sensitive, please email the maintainer at **hajifish** (via GitHub profile contact) before publicly disclosing.

### What to Include

When reporting a vulnerability, please provide:

- A description of the issue and its potential impact
- Steps to reproduce or a proof-of-concept
- Any suggested fixes or mitigations (if applicable)

### Response Timeline

- **Acknowledgment:** We will acknowledge receipt of your report within **48 hours**.
- **Assessment:** We will provide an initial assessment within **7 days**.
- **Resolution:** We aim to release a fix within **30 days** of confirmation, depending on complexity.

### Responsible Disclosure

We kindly ask that you:

- Do not publicly disclose the vulnerability until we have had a chance to address it
- Do not exploit the vulnerability in ways that could harm users or data
- Provide us with reasonable time to fix the issue before any public disclosure

We will credit security reporters in our release notes unless you prefer to remain anonymous.

## Secret Storage & Threat Model

Provider API keys are stored in a local configuration file. Since the encrypted-storage release, that file is a **machine-bound encrypted envelope**: it is encrypted with XChaCha20-Poly1305 (AEAD), and the key is derived via BLAKE3 from this machine's hardware identifier (macOS IOPlatformUUID / Linux machine-id / Windows MachineGuid) together with the app's built-in derivation context. The decryption key is never written to disk.

To avoid overstating our security guarantees, we state the boundaries of this scheme explicitly:

- **What it defends against:** it significantly raises the cost of cracking a copy of the configuration file after it leaves this machine offline — for example when the file is exposed through cloud sync, system backups, or being sent by mistake as an attachment.
- **What it does not defend against:** an attacker who is already logged into the same machine under the same user account. Such an attacker can read this machine's hardware identifier and obtain the derivation parameters from the public source code, thereby recomputing the decryption key offline. This scheme also **does not defend against runtime process-memory reads**.
- Consequently, this scheme is equivalent to narrowing the exposure of plain-text keys from "anyone who gets hold of the file" to "someone who can execute code as you on this machine". It does not provide absolute or unbreakable security.

If you discover any key-disclosure path (including a real-world risk beyond the threat model above), please submit it through the vulnerability reporting process described in this file.

## Supported Versions

Only the latest release of this project receives security updates. Please ensure you are using the most recent version before reporting issues.
