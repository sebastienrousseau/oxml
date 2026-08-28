<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Governance

## Project lead

oxml is maintained by Sebastien Rousseau, who has final say on
technical direction and releases.

## Roles and responsibilities

| Role | Held by | Responsible for |
|---|---|---|
| Project lead | Sebastien Rousseau | Merging changes, cutting releases, responding to vulnerability reports, and deciding scope |
| Contributor | Anyone opening a pull request | The change they propose, and the tests that go with it |

There is no separate reviewer, release-manager or security-officer
role today, because there is one maintainer. Where this document names
a responsibility, the project lead holds it.

## Decision making

Most decisions are made in the open, in issues and pull requests.
Anyone may propose a change. Substantial changes — a new public API, a
new crate in the suite, a change to the versioning policy — should
start as an issue, so the discussion happens before the code.

Changes arrive as pull requests and are merged by the project lead
once CI is green. Disagreements are settled in the pull request or the
issue that prompted it. There is no voting procedure, because there is
no second voter — see *Bus factor* below.

## Bus factor

**The bus factor of this project is one.** That is a statement of
fact, not an aspiration: a single person has commit access, publishes
releases, and holds the crates.io ownership.

The project does not pretend otherwise, and the mitigations are the
ones available to a single-maintainer project:

- Everything needed to build, test and release is in the repository —
  `scripts/gate.sh` runs what CI runs, and `scripts/publish.sh` runs a
  release. Neither depends on a machine only one person has.
- The licence is MIT OR Apache-2.0, so a fork needs no permission.
- Every release is tagged and signed, so a successor can establish
  what shipped and when.

## Access continuity

| Asset | Who holds it | If they are unavailable |
|---|---|---|
| GitHub repository | Project lead | The repository is public; anyone may fork and continue under the licence |
| crates.io ownership | Project lead | A successor publishes under a new name, or the lead adds an owner in advance |
| Release signing key | Project lead | A successor signs with their own key; historical tags remain verifiable |

If the project lead becomes unavailable for an extended period, the
honest expectation is that this repository stops receiving updates.
The public history, tags and documentation are sufficient for someone
else to take it up, and the licence permits it.

## Becoming a maintainer

The project would benefit from a second maintainer, and the criterion
that most obviously blocks a higher OpenSSF Best Practices level is
exactly that. A contributor who lands several substantive changes and
wants commit access should open an issue asking for it. Sustained,
high-quality contribution is the path; there is no fixed threshold.

## Versioning policy

Every member of the oxml suite ships the same version number. A user
reading two different numbers across crates is reading a mistake, not a
deliberate difference.

Versions advance in `0.0.1` steps and stay on the `0.0.x` line;
`0.1.0` follows `0.0.999`. This is checked, not just documented.

## Releases

Releases are cut from `main` with a signed tag. A release requires:

- a green CI run,
- a CHANGELOG entry describing the change,
- the version identical in every file that states it.

`scripts/publish.sh` enforces the first and third before it will
upload, and asks before it does.
