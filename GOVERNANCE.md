# Governance

## Project lead

oxml is maintained by Sebastien Rousseau, who has final say on
technical direction and releases.

## Decision making

Most decisions are made in the open, in issues and pull requests.
Anyone may propose a change. Substantial changes — a new public API, a
new crate in the suite, a change to the versioning policy — should
start as an issue so the discussion happens before the code.

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

## Becoming a maintainer

Sustained, high-quality contribution is the path. There is no fixed
threshold; if you have been reviewing and fixing things for a while,
you will be asked.
