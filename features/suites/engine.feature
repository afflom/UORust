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
  Scenario: The gate checks the committed fixture projection and a fresh projection of this workspace with lexlean and prismpm
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

  @UE-12 @build
  Scenario: A RustSec advisory that affects a locked package's version is a Risk on the supply chain, answered by the advisory gate or accepted where deny.toml ignores it with its reason; one nothing answers is refused, and a run without a database records that none was consulted
    Given the advised fixture, whose Cargo.lock pins smallvec 0.6.9, with five copied RustSec advisories and a deny.toml that ignores RUSTSEC-2018-0018
    When the suite exercises UE-12
    Then Security holds a Risk for each of the four advisories that affect 0.6.9, an AcceptedRisk for the ignored one, failing gate measurements for the rest, and the SDK accepts the projection

  @UE-13 @build
  Scenario: Every feature of this workspace's own projection is admitted: each library module and binary entry point is exercised by a test, doctest or example, directly or through a helper of the test's file
    Given this workspace, whose repo-model codegen and xtask binary have their own tests
    When the suite exercises UE-13
    Then frontier.toml records no feature-without-acceptance refusal, and Interface declares featureRepoModelCodegen and featureXtaskMain

  @UE-14 @build
  Scenario: uorust run on its own source reproduces its hand model: every hand component, call, threat, control and quality measure in app/src is paired with the derived record that states the same fact or listed as outside the code with a reason, the hand route table equals uorust::ROUTES, and the drift is 0
    Given this workspace and the hand model in app/src, with crates/uorust/tests/fixpoint.toml pairing their records
    When the suite exercises UE-14
    Then every paired record on both sides states its fact, every checked hand record is paired or outside, the route tables agree, and the drift is 0

  @UE-15 @build
  Scenario: Iterator chains, inherent methods, read-only maps, early returns and for loops lift
    Given the thermostat fixture, whose roster module uses iterator chains with closures, methods taking self, BTreeMap, HashMap and BTreeSet reads, early returns, let else, tuple lets and for loops that push onto locals
    When the suite exercises UE-15
    Then every roster function and method is a LexLean definition, each chain a helper with recursive_argument, no roster function is residue, and lexlean verify passes on the committed projection

  @UE-16 @build
  Scenario: A lifted function never turns unlifted
    Given this workspace, the thermostat fixture and their committed lift registers
    When the suite exercises UE-16
    Then every function of product code counted from the source has exactly one typed row keyed by its κ, a divergence appears on lifted rows only and the register keeps it, no κ a committed register records as lifted is decided otherwise, this workspace's register is current, a planted function whose callee stops lifting is refused by the register, by uorust write and by uorust check, and a planted unchanged function counted under another construct while its signature still states its own is refused by uorust admit against its parent's register whether or not another function still states it, while moving functions on whose signatures no longer state it, or whose callee lifted, is admitted, an inline module's homonym of a function is a row of its own, and the frontier's type construct row names the file and line of the declaration each name of its chain means where it is written

  @UE-17 @build
  Scenario: The lift translates MIR mechanically
    Given the mir-path fixture, its committed verdict register and the vendored MIR definition of the pinned toolchain
    When the suite exercises UE-17
    Then every MIR kind has a coverage row, the trial projection lists a loop helper with its measure and decrease theorems and a theorem per obligation, std's own MIR instantiated at a call's types, a std model from the table with a mutable borrow returned by its backward function, and a promoted constant lowered from its own body, the mutable borrow of a parameter is a residue row naming its MIR cause, the proved translation lifts, the refused one is residue naming the verifier's diagnostic, and a verdict of another digest admits nothing

  @UE-18 @build
  Scenario: The translator certifies itself and only shrinks by hand
    Given this workspace's lift register and the certified translator's manifest, and the parent commit's
    When the suite exercises UE-18
    Then each lifted translator function is a production root, a root the compiler finds ineligible is refused with its diagnostic, every certified source matches the SHA-256 its bundle records, the share counts the translator functions reachable from the translate entry and those certified, and a commit with more uncertified translator functions than its parent is refused naming each new one and its cause

  @UE-19 @build
  Scenario: Every body of core is embedded and its evaluator checked against rustc
    Given core's MIR dumps, rust-src at the pinned toolchain and the MIR semantics module
    When the suite exercises UE-19
    Then every body is a value of the MIR datatype whose kinds are rustc's, a long list nests no deeper than its root, the constructs the evaluator does not define are read from its source, each register row records its embedding and what blocks it, and each sampled case is a theorem that the evaluator returns what rustc's build returned

  @UE-21 @build
  Scenario: Every numeric literal of a workspace gets a home by extraction and rewrite
    Given small synthetic workspaces and figure registers written for them
    When the suite exercises UE-21
    Then every numeric literal is a site at its place with a key an unrelated edit keeps, the writer expands each figure to its own token, a rewrite names the figure and a second changes nothing, what cannot be placed is refused with its reason, and the gate holds the workspace to its register

  @UE-22 @build
  Scenario: Unconditional exported numeric parameter macros of workspace libraries preserve the surface lift: an absolute invocation through a declared Cargo dependency alias with no arguments and one empty matcher expanding to one numeric literal is read as that literal, with the source facts and function identities unchanged; conditional, ambiguous, argument-taking and unknown macros remain residue
    Given a workspace library and its renamed parameter dependency
    When the suite exercises UE-22
    Then the direct and constant-mediated literal macros lift and other macro forms stay residue

  @UE-23 @build
  Scenario: A library embedding resolves crate-relative implementation spans against the defining crate
    Given two libraries with different implementation headers at the same relative source path and a conflicting root file
    When the suite exercises UE-23
    Then each inherent method resolves only through its own library's source and a missing own source stays unresolved

  @UE-24 @build
  Scenario: An invalid recursive-descent parameter is refused without a panic
    Given a caller with two parameters and descent evidence naming parameter index two
    When the suite exercises UE-24
    Then the result is typed form residue and valid single-parameter evidence still names its own parameter

  @UE-25 @build
  Scenario: A cross-file constant is emitted before its caller
    Given a caller preceding its constant and a mutual group whose member needs a later definition
    When the suite exercises UE-25
    Then dependencies precede their callers and the mutual group remains together without treating string contents as calls

  @UE-26 @build
  Scenario: Inert dependency string data does not taint figure rewrites
    Given a dependency resource that is one string literal and mentions a tracked callee
    When the suite exercises UE-26
    Then inert data introduces no location reader while malformed source remains unknown and real readers remain detected

  @UE-27 @build
  Scenario: Qualified associated functions retain their acceptance roots
    Given two owners with an associated function of the same short name
    When the suite exercises UE-27
    Then each qualified assertion binds its own lifted function and an unknown owner is refused

  @UE-28 @build
  Scenario: MIR method homonyms keep stable qualified names
    Given three modules with equally named conversion methods and their actual compiler MIR
    When every body or only the last module's method is translated on trial
    Then every method has its own candidate and the last method keeps the same declaration name in both views

  @UE-29 @build
  Scenario: Native MIR proof work is bounded by dependency components
    Given dependent translations, mutually recursive translations and a positive batch budget
    When the prover plans and executes its trials
    Then every eligible translation is scheduled once after its dependencies, recursive components remain together, and unchanged proved verdicts need no verifier rerun
    And a verifier diagnostic on a theorem opening names that theorem, never the preceding function
    But zero budgets, dangling dependencies and a recursive component larger than the budget are refused before verification

  @UE-30 @build
  Scenario: An admitted registered alias keeps its model through registry expansion
    Given a package using the locked BigRational alias and its registered BigInt dependency
    When its source is projected with registry aliases available
    Then its identity function lifts as the stored integer pair
    And a mismatched rational archive checksum leaves that function unlifted

  @UE-31 @build
  Scenario: Qualified field types retain their declaring scope
    Given two modules declaring different types with the same short name
    And a holder that uses a qualified type and a conflicting bare imported type
    When the workspace is projected
    Then both field getters lift with the types their written paths identify
    And unresolved or ambiguous paths remain refused

  @UE-32 @build
  Scenario: MIR collection avoids native object generation without losing bodies
    Given private, generic, closure and impl functions compiled by the pinned Rust compiler
    When native emission and forced metadata MIR emission collect the same lowering pass
    Then their body inventories and bytes are identical and nonempty
    But metadata emission without forced MIR encoding is rejected for missing bodies

  @UE-33 @build
  Scenario: Locked JSON data can pass through a formal function without losing its representation
    Given serde_json 1.0.114 pinned to its registered archive checksum
    When identity, tuple and enclosing-field functions are projected
    Then their JSON values retain the six source variants and numeric storage alternatives
    And comparisons without a faithful equality model remain unlifted
    And the actual SDK verifies the carrier definitions and their function bodies
    And recursive JSON native exports are refused individually under the pinned library profile
    And replacing the archive checksum refuses the carrier reading

  @UE-34 @build
  Scenario: Shared mutable owners retain references instead of frozen contents
    Given Rc and Arc owners of RefCell and Cell, including two fields that may alias
    When identity, owner cloning, tuple and enclosing-field functions are projected
    Then their models transport the same allocation references
    And the actual SDK verifies their carrier definitions and function bodies
    But heap allocation, reads, writes, owner counts and value equality stay unlifted
    And native roots requiring these reference carriers are refused individually

  @UE-35 @build
  Scenario: Long type names generate canonical declarations
    Given a structure field and recursive tree with long qualified type names
    When their functions are projected
    Then the pinned SDK accepts the generated modules without rewriting their layout

  @UE-36 @build
  Scenario: Owned slices retain their ordered elements
    Given Box, Rc and Arc slices, including an Rc slice field in a structure
    When identity, length and owner-clone functions are projected
    Then the actual SDK verifies their sequence models
    But reference counts and slices of mutable cells remain unlifted

  @UE-37 @build
  Scenario: A locked bit vector retains logical bits rather than storage words
    Given default bitvec 1.0.1 BitVec pinned to its registered checksum
    When identity and length functions are projected
    Then the actual SDK verifies their Boolean sequence model
    But raw storage and allocation capacity remain unlifted
    And a wrong archive checksum refuses the model

  @UE-38 @build
  Scenario: Native proof refresh reuses observations without trusting stale proofs
    Given SDK-proved translations whose prior records contain observed axiom policies
    When one translation digest is made stale
    Then the actual SDK verifies its refreshed proof in one run using the prior policies as hints
    And unchanged replay uses zero verifier calls
    But incorrect policy hints require corrective SDK diagnostics and another verification

  @UE-39 @build
  Scenario: An alias expands in its defining scope
    Given an alias of nested tuples whose element type has homonyms in other modules
    When the alias is used from a module importing another homonym
    Then its fields retain the original element type and the SDK verifies the model
    And alias type parameters retain their own binding
    But an alias of an unread interior-mutable type stays unlifted

  @UE-40 @build
  Scenario: A renamed import retains its declared type
    Given a unique workspace type imported under another name
    When fields and function signatures use the renamed import
    Then their functions retain the original data type and the actual SDK verifies them
    And a generic type parameter shadowing an import remains generic

  @UE-41 @build
  Scenario: Native aggregates retain their field representations
    Given source structures and native tuples with different signed integer representations
    When their compiler bodies are translated together with an unsigned tuple function
    Then the incompatible aggregates remain typed MIR residue
    And the compatible tuple is translated and its generated fields pass SDK type checking

  @UE-42 @build
  Scenario: Native bodies reuse admitted registry carriers
    Given locked numeric, JSON and default bit-vector dependencies
    When compiler bodies transport their values through loops
    Then their native translations retain the registered representations and the SDK verifies them
    But JSON equality without an operation model is refused
    And a wrong archive checksum prevents the native carrier reading

  @UE-43 @build
  Scenario: Native verification produces scoped reusable feedback
    Given native candidates including an overflowing function
    When the prover submits their trials to the actual SDK
    Then compatible candidates are proved and the overflowing candidate is refused
    And the structured SDK result names only Behavior and its dependency closure
    And unchanged replay performs no new verification and retains the same verdicts

  @UE-44 @build
  Scenario: Loop evidence omits unused invariant arguments
    Given a native countdown loop transporting an invariant Boolean
    When the prover generates its decrease evidence
    Then its live values share one changing loop-state parameter
    And the actual SDK proves the loop without an unbound invariant argument

  @UE-45 @build
  Scenario: Native hash collections reuse unordered carriers
    Given standard maps and sets with default and explicit hasher types
    When compiler-derived countdown loops transport their values
    Then the SDK proves their unordered collection models and a workspace homonym retains its own type
    But hasher inspection, iteration order and unsupported element types stay unlifted

  @UE-46 @build
  Scenario: Compiler-input selection preserves proof history
    Given an SDK-decided register with proved and refused translations
    When the prover replays one compiler body and then all compiler bodies
    Then it retains the other selection's actual verdicts and performs zero verifier runs
    But a live translation with a stale digest still needs actual verification

  @UE-47 @build
  Scenario: Workspace native bodies share primitive IEEE models
    Given ordinary f32 and f64 functions without a core-library scope
    When compiler-derived loops transport floats, compute supported operations and convert their bits
    Then the actual SDK proves their existing IEEE models including negative zero
    And a workspace declaration under a primitive name retains its own type
    But an operation outside the IEEE model stays unlifted

  @UE-48 @build
  Scenario: Early failure preserves mutable state
    Given functions that update mutable parameters before Result or Option propagation
    When propagation succeeds or exits early, including after a state-changing callee
    Then the SDK proves the returned values and every resulting state against executable Rust vectors
    But a mutable alias still refuses the lift

  @UE-49 @build
  Scenario: Native numeric observations use locked stored values
    Given checksum-admitted BigInt and BigRational values
    When compiler-derived bodies observe their signs, identities and stored ratio components
    Then the actual SDK verifies the translations and closed numeric acceptance vectors
    And raw negative or zero denominators retain the locked implementation's behavior
    But a wrong archive checksum and an unsupported operation stay unlifted

  @UE-50 @build
  Scenario: Proof campaigns publish their own measured completion position
    Given compiler inputs and an existing actual SDK verdict register
    When prove writes a requested machine report
    Then it writes the final projection and reports its census, remaining functions and blockers with exact byte bindings
    And unchanged proofs require zero verifier calls without claiming full-project SDK acceptance
    But a foreign projection directory is a typed refusal and its files are preserved

  @UE-51 @build
  Scenario: Numeric constructors derive their reading from word widths
    Given locked numeric crates and compiler-derived primitive integer types
    When constructors convert unsigned values and signed two's-complement words
    Then the SDK verifies every primitive width and sign boundary using bounded binary conversion
    And rational construction preserves the integer as numerator and one as denominator

  @UE-52 @build
  Scenario: Proof decisions follow their semantic dependency context
    Given an actual SDK proof of a native function calling a source-lifted helper
    When that helper changes without changing the caller's own tokens
    Then UORust verifies the changed proof context and reuses its unchanged replay with zero verifier calls
    But a broken shared semantic context is a typed campaign failure rather than per-function refusal

  @UE-53 @build
  Scenario: Bounded native proof closures retain their complete obligations
    Given valid and overflowing native functions beside an unrelated false source acceptance root
    When UORust verifies each native declaration closure
    Then the valid function proves and the actual native overflow remains refused
    And full-project SDK verification still rejects its false source acceptance root
    But an unattributed verifier failure is not cached as an individual counterexample

  @UE-54 @build
  Scenario: Native feedback uses the exact root trial module
    Given a root proof obligation and a lexicographically earlier library Behavior module
    When the SDK points to the root obligation's opening line
    Then UORust attributes the feedback to the root declaration and not the library declaration

  @UE-55 @build
  Scenario: Compiler-generated enum reference aliases retain match evidence
    Given enum and option references copied through tuple patterns and an enum replaced after a match
    When UORust translates the compiler's distinct temporary references
    Then matching aliases reuse the fields of the same typed immutable value
    And executable Rust vectors and SDK theorems agree after replacement
    But a native overflow is still refused

  @UE-56 @build
  Scenario: Signed source fields and native words share explicit integer codecs
    Given source records with mathematical signed integer fields and native word operations
    When the native translator reads, constructs and passes those mathematical signed scalars
    Then actual SDK proofs and Rust boundary vectors agree on negative values and extrema
    But an unrelated Boolean field cannot be interpreted as a signed integer

  @UE-57 @build
  Scenario: Native readonly shared owners use the existing content representation
    Given immutable records owned by standard Rc and Arc pointers
    When native functions construct, clone and read those records
    Then executable Rust and actual SDK boundary vectors agree on their contents
    But reference counts, pointer identity, shared mutable cells and unrelated owners retain typed refusals

  @UE-58 @build
  Scenario: Native unary numeric operations reuse width rules and admitted numeric carriers
    Given primitive signed extrema and checksum-admitted exact integers and raw rational pairs
    When native functions negate, extract magnitude and compute signum
    Then actual SDK vectors agree with Rust including minimum-value wrapping and checked refusal
    And exact rational negation preserves its stored denominator
    But unwrapping a checked negation at the signed minimum cannot be proved

  @UE-59 @build
  Scenario: A blocked partial caller preserves its complete-context proof history
    Given actual SDK proofs of a native caller and its transitive callees
    When a partial compiler selection retains the caller but omits a transitive callee
    Then the caller remains unadmitted without erasing either cached decision
    And restoring complete compiler inputs requires zero verifier calls

  @UE-60 @build
  Scenario: Readonly hash-map operations reuse model-owned lookup semantics
    Given unordered maps with primitive keys and keys with admitted derived equality
    When native code searches shuffled entries or queries their size
    Then actual SDK results agree with Rust for present and absent keys in either entry order
    But custom key equality, hasher observations and unordered iteration remain unadmitted

  @UE-61 @build
  Scenario: Function items reuse their admitted value semantics
    Given admitted immutable workspace functions and standard Rc or Arc content borrows
    When native Option mapping passes those functions as values
    Then SDK-verified results agree with executable Rust for Some and None
    But mutable signatures, custom owners and incompatible targets remain unadmitted

  @UE-62 @build
  Scenario: Structural delivery safeguards reject inefficient or unsupported campaigns
    Given a complete registry and a pinned model runtime authority
    When a delivery campaign is preflighted against its exact inputs and prior receipts
    Then leaf plans, partial scopes and unchanged failures are rejected before work
    And verified identical work reuses its intact receipt
    But a changed artifact or unsupported completion claim is rejected

  @UE-63 @build
  Scenario: Compiler control-flow regions compose without cyclic continuations
    Given nested compiler loops with immutable frames and changing value state
    When UORust derives region boundaries and composes their exit continuations
    Then actual SDK results agree with executable Rust across nested and triple loops, early returns and distinct exits
    And overflow obligations still refuse an unsafe candidate
    But irreducible regions or untransportable borrows remain typed refusals

  @UE-64 @build
  Scenario: Compiler calls import shared source semantic models
    Given compiler-owned standard calls with exact immutable source and native type bindings
    When native lowering imports the source translator's existing semantic declarations
    Then actual SDK results agree with executable Rust across unrelated standard receiver families
    But partial panic models, divergent readings, mutable inputs and custom owners remain refused

  @UE-62 @build
  Scenario: Unchanged SDK dependency closures retain their verified receipt
    Given an intact prior actual SDK verification and a newly SDK-built projection
    When producer bytes change but the SDK build, source, semantic, module and SDK-lock identities agree
    Then the proof stage reuses the original verification with explicit provenance
    But build-only output, changed SDK inputs, modified artifacts or missing proof bundles require actual verification

  @UE-64 @build
  Scenario: Immutable compiler closure environments share the semantic callable interface
    Given compiler closure bodies with exact signatures and immutable captured schemas
    When source and native standard models consume the callback values
    Then actual SDK results agree with executable Rust for captured predicates, mapped values, lazy defaults and function items
    But mutable captured state and a callback overflow cannot acquire a proved verdict

  @UE-65 @build
  Scenario: Native transport reuses admitted source type models
    Given source-admitted semantic types nested in compiler parameters and containers
    When native lowering imports their closed readings after its primitive ABI rules
    Then actual SDK transport properties agree with Rust even while a shared cell is mutably borrowed
    But contents, synchronization, owner counts and pointer comparisons require their own admitted semantics

  @UE-66 @build
  Scenario: Compiler dependencies reuse generic source definitions and dictionaries
    Given source-lifted generic definitions and coherent trait implementation signatures
    When compiler callers pass values through those definitions after native control flow
    Then actual SDK results agree with Rust across parametric transport, containers and trait dictionary calls
    But an incompatible ABI or an unavailable implementation cannot acquire a proved verdict

  @UE-67 @build
  Scenario: Parametric compiler bodies preserve generic product state
    Given declared Rust type parameters and compiler bodies with natural loops and product returns
    When the shared lowering binds the generated definitions and their dependencies
    Then actual SDK results agree with Rust at multiple instantiations and nested control-flow regions
    But generic drop effects and unresolved trait dictionary bodies remain typed residue

  @UE-68 @build
  Scenario: Native proof identities follow their actual model dependency closure
    Given compiler candidates with reached source definitions and schemas
    When an unrelated source definition changes without changing those dependencies
    Then their rooted model identities and intact original decisions are reused
    But changing a reached definition or schema requires a new actual SDK decision

  @UE-69 @build
  Scenario: Compiler products preserve opaque fields without inventing their operations
    Given complete declared structs containing known fields and unavailable service values
    When native lowering composes their dependency views through SDK products
    Then known field access and update agree with executable Rust while retaining opaque payloads
    But opaque methods, incomplete shapes and unmodeled drop effects remain typed refusals

  @UE-70 @build
  Scenario: Compiler writes preserve declared storage carriers
    Given compiler field and container writes with distinct source and native carriers
    When lowering transports the incoming type through nested place updates
    Then admitted representation codecs preserve executable Rust results and stored values
    But an incompatible opaque slot is refused before an ill-typed model reaches the SDK

  @UE-71 @build
  Scenario: Parametric container dependencies share canonical product entries
    Given generic maps transported and read by source and compiler bodies
    When lowering binds their entry representation and dependency declarations
    Then actual SDK models elaborate and agree with executable Rust at distinct instantiations
    But unavailable key comparison evidence remains a typed refusal

  @UE-72 @build
  Scenario: Compiler constant operands reuse admitted source meanings
    Given source constant models with distinct target and module identities
    When native bodies read the constants through the shared dependency interface
    Then actual SDK results agree with executable Rust and bind the reached initializers
    But unavailable initializers and incompatible carriers remain typed refusals

  @UE-73 @build
  Scenario: Source signed fields share the width-derived native word codec
    Given source models of signed integer fields at every supported fixed width
    When compiler reads cross the shared representation boundary
    Then actual SDK results agree with executable Rust at sign and range boundaries
    But incompatible widths and unavailable reverse codecs remain typed refusals

  @UE-74 @build
  Scenario: Alternating dependency contexts retain their own native receipts
    Given actual SDK decisions for the same Rust question in distinct bound contexts
    When the full campaign and a dependency view alternate
    Then each view reuses its intact prior decision without a new verifier run
    But a changed dependency or a corrupted context cannot acquire either decision

  @UE-75 @build
  Scenario: Native words reuse signed source carriers across all fixed widths
    Given compiler calls, constructors and mutations crossing signed integer model boundaries
    When the common codec composes the SDK bit operations through balanced shared kernels
    Then actual SDK results and final state agree with executable Rust at every fixed width
    But incompatible widths and unrelated stored types remain typed refusals

  @UE-76 @build
  Scenario: Integer carriers compose through nested structures
    Given compiler calls and state crossing nested source and native collection carriers
    When UORust composes admitted element codecs through the complete structural type
    Then the SDK agrees with Rust while preserving nested values and entry order
    But incompatible element models and collection identities remain refused

  @UE-77 @build
  Scenario: Parameterless source models remain applications at every compiler boundary
    Given compiler calls returning values from admitted parameterless source definitions
    When UORust materializes their calls through the common application boundary
    Then the SDK agrees with Rust for returned values, constructed fields and mutable final state
    But changed source meanings cannot reuse the previous native decisions

  @UE-78 @build
  Scenario: Overlapping method names retain distinct compiler definition identities
    Given trait implementations and an inherent method sharing an owner and method name
    When their compiler bodies are joined to registered source definitions
    Then each unique implementation source site receives its own SDK-verified behavior
    But duplicate compiler identities cannot be resolved by choosing the first body

  @UE-79 @build
  Scenario: Compiler calls retain coherent trait definition identities
    Given registered overloaded trait and inherent definitions
    When compiler callers use resolved qualified trait paths
    Then every caller selects its own registered semantic definition
    And unrelated same-named traits cannot substitute for that definition

  @UE-80 @build
  Scenario: Shared kernels preserve their original SDK observations across owners
    Given identical generated kernels owned by multiple native functions
    When one owner carries an exact matched SDK axiom observation
    Then the observation survives either owner emission order
    But a different kernel body cannot inherit that observation

  @UE-81 @build
  Scenario: Existing semantic models serve receiver and associated compiler calls
    Given existing source models for immutable standard values
    When compiler references use either a receiver or an associated call
    Then the common importer materializes the existing source meaning
    But unrelated namespaces and incompatible state interfaces remain refused

  @UE-82 @build
  Scenario: Imported model dependency closures include every declared type form
    Given an imported model over algebraic and qualified prelude values
    When native type resolution checks the model dependency closure
    Then the existing declared types remain available through one lookup
    But a term declaration or undeclared nominal type is not a type authority

  @UE-83 @build
  Scenario: Recursive regions carry mutable-call results in their lexical frame
    Given a compiler loop that calls a source-modeled mutable method
    When its updated state flows into the next recursive region invocation
    Then one evaluated call result is bound in the recursive evidence scope
    And SDK value checks and termination evidence accept the whole loop

  @UE-84 @build
  Scenario: Observed policy retries reuse the same projected dependency closure
    Given a fresh native proof campaign with an SDK-observed policy correction
    When no semantic decision or dependency changes between verifier attempts
    Then the existing projected package supplies the next SDK attempt
    And unchanged statement and proof bytes remain bound to the same candidates

  @UE-85 @build
  Scenario: Generated implementation dependencies import their actual compiler bodies
    Given compiler-generated methods without handwritten impl headers
    When the pinned compiler records qualified identities beside their MIR
    Then the shared dependency importer binds and verifies their actual bodies
    But stale input bindings and ambiguous definition origins are refused

  @UE-86 @build
  Scenario: Source field identities constrain compiler value transport
    Given complete source views with abstract field carriers and actual compiler bodies
    When field projections and value transport constrain compiler locals and results
    Then signatures and lowering use the same source-bound type environment
    But conflicting identities and unknown operations or effects remain refused

  @UE-87 @build
  Scenario: Equal compiler types specialize existing source carrier frames
    Given complete source views with independently bound opaque fields
    When the compiler transports an identical resolved type between the fields
    Then the native signature and body share one specialized carrier frame
    But unrelated types and unsupported operations remain refused

  @UE-88 @build
  Scenario: A single source constructor supplies shared field projections
    Given admitted source data with exactly one constructor and compiler field reads
    When rustc omits an unnecessary discriminant branch
    Then native reads apply shared typed projection kernels verified by the SDK
    But multiple constructors and incompatible field readings supply no projection

  @UE-89 @build
  Scenario: Parametric source calls preserve native payload representations
    Given compiler-qualified standard calls over native words or abstract payloads
    When an existing source model only transports those payloads through callbacks
    Then a shared generic kernel imports that model through exact type bindings
    But arithmetic without a payload model and mutable schemas remain refused

  @UE-90 @build
  Scenario: Source shapes resolve compiler aggregate constructor forms
    Given admitted struct and enum declarations with named and positional fields
    When compiler MIR constructs a record-shaped enum variant or qualified tuple struct
    Then native construction preserves the actual constructor and declared field order
    But missing, duplicate and incompatible field readings remain refused

  @UE-91 @build
  Scenario: Callbacks and native auxiliaries share complete declaration ordering
    Given compiler callbacks whose auxiliary bodies reference native definitions
    When source and native declarations are emitted and proof dependencies sliced
    Then function values and direct calls share one provider-before-consumer order
    And quoted content and indivisible mutual groups retain their existing behavior

  @UE-92 @build
  Scenario: Existing source numeric models cross the native representation boundary
    Given compiler-qualified calls whose native words differ from source integer carriers
    When both input and result representations have existing exact codec kernels
    Then source semantics are imported through those shared kernels
    But mutable interfaces and unavailable carrier conversions remain refused

  @UE-93 @build
  Scenario: Closed primitive trait identities reach their source models
    Given compiler-qualified standard trait calls on primitive owners
    When a source semantic model and exact ABI kernels already exist
    Then the existing semantics bind through the shared import mechanism
    But workspace traits and owners with unbound payload implementations remain refused

  @UE-94 @build
  Scenario: Captured compiler context binds pointer-sized semantic imports
    Given MIR with its exact compilation target and matching compiler layout
    When an existing source model needs the pointer width of that compilation
    Then fixed-width semantic and codec kernels use that recorded ABI
    But missing or conflicting target evidence supplies no specialization

  @UE-95 @build
  Scenario: Compiler panic paths preserve their computed outcomes and value-state
    Given a native Rust body whose valid inputs may fail a compiler assertion
    When the exact normal-only model was rejected and the value-state ABI is available
    Then the canonical function computes both return and panic outcomes with the updated state
    But cleanup and unavailable effect interfaces cannot be discarded to admit it

  @UE-96 @build
  Scenario: Native dependency closures preserve panic outcomes and mutable state
    Given a chain of native calls whose leaf can fail a compiler assertion
    When the shared value-state outcome ABI is selected for that dependency closure
    Then every supported caller propagates the actual outcome and updated state
    But a partial function cannot be erased into a pure function-value interface

  @UE-97 @build
  Scenario: Source-owned constant namespaces retain their declaring type
    Given closed inherent constants on two types with the same item name
    When native MIR refers to their source-owned values and an aggregate typed Self
    Then the existing source initializer kernels supply each owner's exact value
    But generic, trait and unresolved owner interfaces supply no model

  @UE-98 @build
  Scenario: Native dependency interfaces retain memory on return and panic
    Given native calls using the existing allocation and pointer model
    When their dependency closure requires memory and computed assertion outcomes
    Then each caller passes and returns that exact state and frame-local cells are released on either exit
    But unrelated allocations and unavailable cleanup or borrow interfaces cannot be discarded

  @UE-99 @build
  Scenario: Locked registry calls reuse their existing source semantics
    Given compiler calls on a registry type admitted by its exact Cargo lock
    When an inherent method or language operator has an existing source model
    Then the native adapter imports that model through its exact typed interface
    But unrelated traits and unavailable or mutable interfaces supply no meaning

  @UE-100 @build
  Scenario: Compiler primitive namespaces retain their existing kernels
    Given compiler-owned qualified primitive type and inherent owner paths
    When native MIR reads their constants and existing unary operations
    Then the same primitive kernels supply the captured types and values
    But workspace lookalikes and target-dependent identities cannot select them

  @UE-101 @build
  Scenario: Constructor function values reuse admitted enum schemas
    Given compiler function items naming tuple variants of admitted enums
    When a native dependency passes them to an existing higher-order model
    Then shared callable kernels construct those exact variants through the established aggregate lowering
    But unavailable schemas and incompatible carriers cannot acquire a callable meaning

  @UE-102 @build
  Scenario: Native generic bodies reuse source trait dictionaries
    Given an admitted source trait dictionary and compiler-qualified dispatch in a native body
    When the existing generic binder supplies each concrete implementation
    Then native calls and loop helpers retain that exact typed dictionary
    But unrelated traits and unavailable dictionary interfaces cannot supply a method

  @UE-103 @build
  Scenario: Compiler-library type schemas reuse their source declaration graph
    Given closed standard-library types in captured native MIR and the exact pinned Rust source archive
    When the existing declaration instancer imports their reachable schemas
    Then native construction and field reads retain the compiler-owned fields and carriers
    But incompatible, conditional and unbound schemas cannot acquire a model

  @UE-104 @build
  Scenario: Native calls reuse the pinned compiler-library body graph
    Given native calls into the exact compiler-library dependency graph
    When the existing library resolver instantiates reachable compiler bodies
    Then shared native definitions agree with Rust across record and primitive interfaces
    But altered identities and unavailable or incompatible bodies cannot supply a definition

  @UE-105 @build
  Scenario: Compiler optimization preserves borrowed library state before lifting
    Given compiler-library dependencies over closed native records
    When the pinned compiler optimizes their source-bound MIR graph
    Then native library calls preserve Rust return values and borrowed record updates
    But incompatible producer or interface bindings cannot supply semantics

  @UE-106 @build
  Scenario: Source aggregates follow imported compiler schemas
    Given a source aggregate containing a compiler-library field type
    When source and native declarations are merged into one behavior module
    Then its field types precede their consumers and SDK verification succeeds
    But unavailable types, cycles and conflicting declarations remain rejected

  @UE-107 @build
  Scenario: Optimized MIR arithmetic shares modular intrinsic kernels
    Given plain wrapping integer operations exposed by pinned compiler optimization
    When native lowering binds their existing fixed-width intrinsic kernels
    Then Rust and SDK observations agree across unsigned and signed boundaries
    But explicit overflow assertions and unchecked-operation preconditions remain enforced

  @UE-108 @build
  Scenario: Compiler reinterpretation forms share admitted representation kernels
    Given typed reinterpret casts and compiler intrinsic calls
    When both forms bind the same native representation kernel
    Then pointer carriers and UTF-8 and fixed-width views retain Rust observations
    But invalid scalar values, invalid UTF-8 and unavailable layouts remain refused

  @UE-109 @build
  Scenario: Native proof roots reuse one immutable dependency index
    Given multiple proof roots in one combined declaration model
    When native context identities select their dependency closures
    Then one lexical parse supplies byte-identical independent closures
    But malformed models and missing or empty root sets remain rejected

  @UE-110 @build
  Scenario: Native record schemas precede source-only carrier fallbacks
    Given a closed compiler-owned record with a source carrier and no native constructor
    When the pinned declaration importer binds its native interface
    Then its constructor and field reads retain the authoritative declared field order
    And native models retain priority while ambiguous unbound and foreign schemas are refused

  @UE-111 @build
  Scenario: Imported compiler bodies close their record dependencies
    Given consumer and canonical paths for closed compiler record dependencies
    When an imported body introduces another closed compiler-owned record
    Then the strict schema importer retains exact identities and complete declarations
    But foreign ambiguous and unbound schemas remain refused

  @UE-112 @build
  Scenario: Immutable compiler graphs reuse exact linkage queries
    Given repeated positive and negative linkage queries against one immutable compiler graph
    When their callee argument types and destination are unchanged
    Then name normalization and body inference run once for each complete query
    But changed query fields and distinct graph owners retain independent results

  @UE-113 @build
  Scenario: Resolved workspace types survive external glob overlays
    Given a qualified parent type reexport used beside an unrelated external glob
    When source field and signature types pass through scoped registry rewriting
    Then the resolved declaration retains its workspace identity
    But genuine foreign unresolved and shadowed types remain distinct

  @UE-114 @build
  Scenario: Bounded proof batches reuse their immutable model catalogue
    Given independent native proof roots that exceed one proof batch
    When the exact source compiler SDK and model inputs remain unchanged
    Then the batches share one whole-workspace catalogue projection
    And each selected dependency closure passes the actual SDK verifier
    But semantic refusals invalidate the catalogue before changed models are retried

  @UE-115 @build
  Scenario: Retained library captures supply native dependencies beyond core
    Given a compiler-owned alloc call absent from the preferred core graph
    When the retained Factory capture source and target bindings are exact
    Then its body is imported through the same native ABI and proof kernels
    But changed captures foreign owners and incompatible interfaces remain refused

  @UE-116 @build
  Scenario: Empty constructor results contain no payload state
    Given a zero-input source kernel whose container payload can hold mutable state
    When its emitted result is the exact literal empty container of the native ABI
    Then the empty result is admitted without reading or constructing a payload value
    But mutable inputs and nonempty result kernels remain refused

  @UE-117 @build
  Scenario: Compiler normalization preserves admitted caller interfaces
    Given a native dependency whose generic argument contains an admitted caller type
    When compiler declarations normalize that argument to a library-local path
    Then the imported body retains the same argument result and mutable state readings
    But unknown foreign types and conflicting readings remain refused

  @UE-118 @build
  Scenario: Imported returned borrows retain their backward interface
    Given a compiler-owned dependency that returns a mutable borrow
    When its exact body admits both native forward and backward kernels
    Then a write through the returned borrow updates the caller's original state
    But incompatible ABIs and unadmitted memory and panic borrowed interfaces remain refused

  @UE-119 @build
  Scenario: Imported generic bodies dispatch to admitted consumer implementations
    Given a compiler-owned generic wrapper that calls a concrete consumer trait implementation
    When the source identity resolver selects one registered implementation
    Then the imported body reuses that implementation through its admitted native interface
    But foreign ambiguous and incompatible identities remain refused

  @UE-120 @build
  Scenario: Local annotations share authoritative alias expansion
    Given aliases read from exact locked dependency archives and resolved workspace declarations
    When source bodies read those aliases in local and nested annotations
    Then they use the same bounded substitution as signatures and fields
    But generic parameters lexical shadows incompatible arity and unverified archives gain no alias reading

  @UE-121 @build
  Scenario: Compiler constants retain their complete dependency capture
    Given an exact Factory core capture containing constants absent from the preferred graph
    When a closed query names a source-verified unconditional constant provider
    Then its body and dependencies use the existing native kernels in their owning graph
    But foreign overridable mismatched-target and changed-capture bindings remain refused

  @UE-122 @build
  Scenario: Enum tag observations retain the value present at the compiler read
    Given a compiler body that reads a payload enum tag before replacing the enum
    When the saved tag is used as an integer
    Then the SDK observes the original variant under the compiler integer representation
    But an observation of the replacement tag as the original is rejected

  @UE-123 @build
  Scenario: Closed generated module calls import the captured compiler dependency closure
    Given different path modules with identically named generated functions
    When registered callers reach the functions and their private dependencies
    Then SDK observations agree with the corresponding compiled Rust module
    But an incompatible module identity cannot supply its sibling's result

  @UE-124 @build
  Scenario: A native residue selects another compiler representation of the same function
    Given paired compiler bodies with a common source identity and interface
    When normal lowering of the primary body leaves a residue
    Then the runtime body can enter the same dependency and SDK proof gates
    But a changed compiler body cannot supply another function's implementation

  @UE-125 @build
  Scenario: A missing transitive call selects a closed compiler representation
    Given a lowered caller whose native dependency has no admissible body
    When a pinned compiler representation removes the unavailable dependency
    Then the graph selects the closed representation for the ordinary SDK trial
    But a provider with another missing transitive call cannot close the residue
