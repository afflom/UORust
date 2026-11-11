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
    Given a workspace whose call is ambiguous, whose enum is both an error and a state, whose functions are untested and which declares no rust-version
    When the suite exercises UE-03
    Then frontier.toml records ambiguous-call, ambiguous-fact, feature-without-acceptance and no-msrv refusals with status open
    And the Interface module declares no feature

  @UE-04 @build
  Scenario: Total functions over bool, unsigned comparisons and unit enums lift to LexLean definitions, closed assertions over them become decide theorems, and every other function is residue with a typed reason
    Given the fixture workspace
    When the suite exercises UE-04
    Then Behavior defines shouldHeat, withinBand, nextMode and halt with decide theorems from the fixture's assertions
    And frontier.toml records midpoint as arithmetic residue and parse_target as type residue

  @UE-05 @build
  Scenario: The engine's router equals model/routes.toml route for route and answers every router vector of that table exactly
    Given the route table in model/routes.toml
    When the suite exercises UE-05
    Then uorust::ROUTES lists the same routes in the same order
    And uorust::respond answers each non-workbench vector with its response

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

  @UE-08 @build
  Scenario: A public trait's methods are interface calls its module's feature names, and a method call through the trait resolves to the trait's declaration
    Given the fixture workspace, whose sensor module declares pub trait Sensor implemented by Fixed
    When the suite exercises UE-08
    Then Interface declares callThermoCoreSensorSensorRead and callThermoCoreSensorSensorCalibrated, and featureThermoCoreSensor names both and is accepted by the test that calls Fixed(20).read()

  @UE-09 @build
  Scenario: Each module with processes is its own OPM view beside its component's: the component is the object, the module's functions are its processes, and each process zooms into the processes it calls in the module
    Given the fixture workspace, whose config module's load_target calls parse_target
    When the suite exercises UE-09
    Then Design declares the view opmModuleLibThermoCoreConfig with an invocation and a zoom from load_target to parse_target, and one view per other module

  @UE-10 @build
  Scenario: The Behavior and SupplyChain modules take their titles from uorust.titles, a glossary package the projection carries and locks
    Given the fixture workspace
    When the suite exercises UE-10
    Then Behavior is titled Lifted Rust behavior and SupplyChain Locked supply chain, both use uorust.titles@1.0.0, and lexlean.toml and lexlean.lock name language/uorust.titles

  @UE-11 @build
  Scenario: Unsigned arithmetic whose overflow the code names, signed integers, strings, structs, tuples, Option, Result with ?, slices with structural recursion, integer matches and string format! lift to LexLean definitions that lexlean verify accepts, with the Lean axioms they need declared
    Given the fixture workspace, whose band module uses checked, wrapping and saturating arithmetic, i64, &str, a struct, a tuple, Option, Result, a slice and format!
    When the suite exercises UE-11
    Then every band function is a Behavior definition, plain + in midpoint stays arithmetic residue, and the uorust recipe runs lexlean verify on both projections
