# Security

## Reporting

Report a vulnerability privately through
[GitHub security advisories](https://github.com/sisaio/sisa-telemetry-rs/security/advisories/new),
not in a public issue.

## Supported versions

| Version | Security fixes |
| --- | --- |
| The latest 0.x minor release | Yes, as a patch release of that minor |
| Older releases | No: upgrade to the latest minor |

## After you report

- You get a first answer within 7 days.
- We confirm the problem with you, agree on a fix, and keep the advisory private until the fixed
  release is on crates.io.
- The fix ships as a patch release, and the advisory is published with it, crediting you unless
  you'd rather not be named.

## What counts

A value from `otlp.headers` reaching a log line, an error message or `Debug` output is a
vulnerability, as is anything this library puts into a span, a metric or baggage that the
application didn't give it. What an application itself records, such as a request body in a span
field or a secret in baggage, is the application's choice, not a vulnerability here; the
[instrumenting guide](docs/instrumenting.md#7-what-never-goes-into-telemetry) says what to keep
out.

## Verifying a release

Each GitHub release carries the crates' `.crate` files as crates.io serves them, checked against
a packaging of the tagged source, and a Sigstore bundle with their SLSA build provenance, made by
the release workflow. Check a downloaded crate with the GitHub CLI:

```sh
gh attestation verify sisa-telemetry-<version>.crate --repo sisaio/sisa-telemetry-rs \
    --signer-workflow sisaio/sisa-telemetry-rs/.github/workflows/release.yml
```
