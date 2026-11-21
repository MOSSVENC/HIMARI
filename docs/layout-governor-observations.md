# `dsh-layout-governor` observations

Running notes on the directory-governance plugin (`layout_audit`, engine
`dsh-layout-governor` 0.2.0) as observed on 2026-11-20. Each item is reproduced
against a real call, not inferred from documentation.

## L1 — unconfigured tree reports `overall: "pass"`

A tree without `layout.policy.yml` audits as a **pass**.

```text
root=/tmp/lgprobe/empty   policy.present=false
rules: 7/7 enabled=false  findings=0  counts.fail=0
overall: "pass"
```

`pass` is indistinguishable from "audited and clean", so this result cannot
gate anything: forgetting the policy file looks identical to a clean tree. An
unconfigured root should report a distinct state (for example
`not-configured` / `skipped`) or fail.

## L2 — `policy.sha256` is reported for a policy that does not exist

Two unrelated roots with no policy file both report the same digest:

```text
/tmp/lgprobe/empty/layout.policy.yml   sha256 42faa9206bef6e7d6c73b207ae52f28cf160afe481bcb822503c022115328cf7
/home/mossvenc/Project/RAM-CONTROL-CENTER/layout.policy.yml
                                       sha256 42faa920…28cf7
```

The digest is evidently the built-in default policy, but it is printed under
`policy.path`, a file that is absent (`present: false`). Two consequences:

- the value implies a read that never happened;
- it cannot be used for policy drift detection, because the reported digest
  does not change when the file appears.

Either label it as the effective built-in policy (`source: "default"`) or leave
`sha256` null when `present` is false.

## L3 — command guard blocks writes outside the governed root

Hard block, reproducible:

```bash
cd /tmp/lgprobe && mkdir -p poc4 && cd poc4 && echo x > f.txt
```

```text
Error: layout policy: "poc4" is not a declared top-level entry of
/home/mossvenc/Project/RAM-CONTROL-CENTER, and it does not exist yet.
```

The same chain without the trailing relative `cd` passes:

```bash
cd /tmp/lgprobe && mkdir -p poc3 && echo made     # ok
```

Properties of the false positive:

- the tree being written (`/tmp/lgprobe/poc4`) is **outside** the audited root,
  yet the error is raised against the workspace root;
- the guard reports a *creation* for a step that only changes directory;
- a leading absolute `cd` in the same chain does not move the resolution base;
- an intervening command (`cd … && ls >/dev/null && cd nested`) does not break
  resolution either, so the trigger is specifically `mkdir X` followed by
  `cd X`.

Minimum fix direction: track the effective cwd across the whole chain, and only
validate paths that resolve inside the audited root.

## L4 — `hashMode: "full"` with nothing hashed

```text
stats.hashMode = "full"
stats.hashedFiles = 0
stats.hashedBytes = 0
```

Measured on both probes, including one with a single 1-byte file. The label
claims full hashing while no file was hashed, and the `duplicates` rule depends
on those hashes. Either hash and report it, or report `hashMode: "none"`.

## L5 — the walk includes `.git` and build output, and calls them canonical

On this repository:

```text
entries 109,855   files 84,401   bytes 76,056,041,637   durationMs 3393
target/   76,034,156,907 bytes   83,009 files   class=canonical
.git/      4,538,435 bytes        1,190 files   class=canonical
```

`target/` and `.git/` are regenerable or repository metadata, not canonical
content, and `.gitignore` lists `/target`. The `regenerable` class exists in the
report but stays at `0 bytes / 0 files`. Consequent effects: "large files" and
"duplicate payload" rules are dominated by build output, and a single audit walks
76 GB. Honor `.gitignore` and assign regenerable paths to the regenerable class.

## L6 — unknown rule ids are silently accepted

```text
rule=["nope","top-level"]  ->  normal report, no warning
```

A mistyped rule id therefore silently narrows or empties the audit while the
report still looks authoritative. Reject unknown ids.

## L7 — inconsistent, raw errors

```text
root=/tmp/lgprobe/does-not-exist
Error: ENOENT: no such file or directory, stat '/tmp/lgprobe/does-not-exist'

root=/tmp/lgprobe/empty/a.txt
Error: audit root is not a directory: /tmp/lgprobe/empty/a.txt
```

The first leaks the runtime (`fs.stat`) and gives no guidance; the second is a
purpose-written message. Both should use one error shape, and neither says
whether a policy was consulted.

## L8 — "disabled" and "ran clean" are indistinguishable per rule

Every rule reports `durationMs: 0` and `coverage: {}` whether it was skipped or
executed cleanly. Add an explicit `skipped`/`executed` marker per rule.

## L9 — the effective policy is not exposed

The command guard enforces a built-in `top-level.allow` list, but `layout_audit`
never prints it: `policy.present: false` plus a hash is all that is returned. A
user who is blocked has no way to discover which names are permitted without
reading the plugin source. Return the effective allowlist (or at least
`policy.source` and its top-level entries).

## Notes for the maintainer

- L1 and L2 together mean an audit report can look green and current while no
  policy exists and no hash was taken. Those two are the highest-value fixes.
- L3 is the one that actively blocks work: it stopped a write into `/tmp`.
- L5 is why a quick "why did this drift" question turns into a 76 GB scan on a
  Rust workspace.

Quoted tool output above is kept verbatim from before the project was renamed to `HIMARI`; the paths and the crates it mentions are historical.

The parent directory was later renamed to `/home/mossvenc/Projects`, with `/home/mossvenc/Project` kept as a symlink so recorded workspace paths still resolve. The quoted paths above are left as the plugin printed them.
