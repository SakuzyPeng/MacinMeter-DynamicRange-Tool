[English](LEGAL.md) | [中文](LEGAL_CN.md)

# Legal notes and acknowledgements

MacinMeter is an independently written Rust project released under the MIT
License. Its current reference target is foobar2000 DR Meter 1.0.8
(`foo_dr_meter`) by Janne Hyvärinen; the original plugin and its source
code are not included in this repository.

The project maintainer has obtained the author's permission to reverse engineer
the plugin. The private correspondence is retained outside the public
repository. Public engineering records should contain only the minimum
permission summary needed to explain the research boundary; they must not
publish private messages without separate consent.

Permission to study a target does not prove that this implementation matches
it. MacinMeter reports
`foo_dr_meter 1.0.8 Candidate V1 / Unverified`, and the project makes no
reference-compatibility, certification, endorsement, or “official result”
claim. The target name and version identify the subject of the evidence; they
do not imply affiliation or parity.

## Clean implementation boundary

Project work should:

- use legally obtained target binaries and tools;
- record target identity, hashes, host version, and experiment conditions;
- distinguish observable results, hypotheses, and implementation decisions;
- avoid copying original source code or redistributing target binaries without
  separate authorization;
- avoid using names or presentation in a way that implies endorsement.

Private authorization material and non-redistributable binaries do not belong
in `reference/`. That directory contains only public specifications,
reproducible experiment definitions, permitted fixtures, observations, and
conformance summaries.

## Third-party software

Rust and frontend dependencies remain under their respective licenses. See
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md) and the lockfiles for the
release dependency set. A distributable release should include a license report
generated from the exact locked dependency graph.

## Disclaimer

This file documents the project's engineering policy and known authorization
context; it is not legal advice and does not make a general claim about reverse
engineering law in every jurisdiction. Contributors and distributors are
responsible for obtaining professional advice where their use requires it.
