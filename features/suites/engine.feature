Feature: engine

  uorust reads a Cargo workspace and writes a Prism project: router proposals,
  admission that refuses ambiguity with typed refusals, and a byte-deterministic
  projection the PrismPM SDK checks.

  @UE-01 @build
  Scenario: uorust projects the fixture workspace into exactly the committed Prism project, which lexlean fmt, lexlean lock and prismpm check accept
    Given the fixture workspace in crates/uorust/tests/fixtures/thermostat
    When the suite exercises UE-01
    Then the fresh projection equals crates/uorust/tests/fixtures/thermostat-prism byte for byte

  @UE-02 @build
  Scenario: Two projections of the same workspace are byte-identical, for the fixture and for this workspace
    Given the fixture workspace and this workspace
    When the suite exercises UE-02
    Then each workspace projects to the same files with the same bytes twice

  @UE-03 @build
  Scenario: Admission refuses an ambiguous call, a feature without acceptance and a missing MSRV with typed refusals recorded as open claims, and emits no record for them
    Given a workspace whose call is ambiguous, whose functions are untested and which declares no rust-version
    When the suite exercises UE-03
    Then frontier.toml records ambiguous-call, feature-without-acceptance and no-msrv refusals with status open
    And the Interface module declares no feature

  @UE-04 @build
  Scenario: Total functions over bool, unsigned comparisons and unit enums lift to LexLean definitions, closed assertions over them become decide theorems, and every other function is residue with a typed reason
    Given the fixture workspace
    When the suite exercises UE-04
    Then Behavior defines shouldHeat, withinBand, nextMode and halt with decide theorems from the fixture's assertions
    And frontier.toml records midpoint as arithmetic residue and parse_target as type residue

  @UE-06 @build
  Scenario: Every way a run can end without a projection is a typed UorustError variant with a negative test
    Given a missing workspace, a directory with no manifest, a malformed lockfile, a bad timestamp, a foreign output directory and a stale projection
    When the suite exercises UE-06
    Then each ends in its own UorustError variant

  @UE-07 @build
  Scenario: just vv checks the committed fixture projection and a fresh projection of this workspace with lexlean fmt --check, lexlean lock --check and prismpm check
    Given the Justfile
    When the suite exercises UE-07
    Then vv runs the uorust recipe, which runs the three SDK checks on both projections
